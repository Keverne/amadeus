#![doc(html_root_url = "https://docs.rs/amadeus-postgres/0.2.0")]
#![feature(specialization)]
#![feature(type_alias_impl_trait)]

mod impls;

#[doc(hidden)]
pub use postgres as _internal;

use bytes::{Buf, Bytes};
use futures::{ready, stream, FutureExt, Stream, StreamExt, TryStreamExt};
use pin_project::pin_project;
use postgres::{CopyOutStream, Error as InternalPostgresError};
use serde::{Deserialize, Serialize};
use serde_closure::*;
use std::{
	convert::TryFrom, error, fmt::{self, Debug, Display}, io, io::Cursor, marker::PhantomData, ops::Fn, path::PathBuf, pin::Pin, str, sync::Arc, task::{Context, Poll}, time::Duration
};

use amadeus_core::{
	dist_stream::DistributedStream, into_dist_stream::IntoDistributedStream, util::IoError, Source as DSource
};

const MAGIC: &[u8] = b"PGCOPY\n\xff\r\n\0";
const HEADER_LEN: usize = MAGIC.len() + 4 + 4;

pub trait PostgresData
where
	Self: Clone + PartialEq + Debug + 'static,
{
	fn query(f: &mut fmt::Formatter, name: Option<&Names<'_>>) -> fmt::Result;
	fn decode(
		type_: &::postgres::types::Type, buf: Option<&[u8]>,
	) -> Result<Self, Box<dyn std::error::Error + Sync + Send>>;
}

impl<T> PostgresData for Box<T>
where
	T: PostgresData,
{
	fn query(f: &mut fmt::Formatter, name: Option<&Names<'_>>) -> fmt::Result {
		T::query(f, name)
	}
	default fn decode(
		type_: &::postgres::types::Type, buf: Option<&[u8]>,
	) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
		T::decode(type_, buf).map(Box::new)
	}
}

#[derive(Serialize, Deserialize)]
pub enum Source {
	Table(Table),
	Query(String),
}

#[derive(Serialize, Deserialize)]
pub struct Table {
	schema: Option<String>,
	table: String,
}
impl Table {
	pub fn new(schema: Option<String>, table: String) -> Self {
		Self { schema, table }
	}
}

impl Display for Table {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		if let Some(ref schema) = self.schema {
			EscapeIdentifier(schema).fmt(f)?;
			f.write_str(".")?;
		}
		EscapeIdentifier(&self.table).fmt(f)
	}
}
impl str::FromStr for Table {
	type Err = ();

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if s.contains(&['"', '.', '\0'] as &[char]) {
			unimplemented!(
				"Table parsing not yet implemented. Construct it with Table::new instead"
			);
		}
		Ok(Self {
			schema: None,
			table: s.to_string(),
		})
	}
}

#[derive(Serialize, Deserialize)]
pub struct ConnectParams {
	hosts: Vec<Host>,
	ports: Vec<u16>,
	user: Option<String>,
	password: Option<Vec<u8>>,
	dbname: Option<String>,
	options: Option<String>,
	connect_timeout: Option<Duration>,
}
impl From<ConnectParams> for postgres::config::Config {
	fn from(from: ConnectParams) -> Self {
		let mut config = postgres::config::Config::new();
		for host in from.hosts {
			let _ = match host {
				Host::Tcp(addr) => config.host(&addr.to_string()),
				Host::Unix(path) => config.host_path(&path),
			};
		}
		for port in from.ports {
			let _ = config.port(port);
		}
		if let Some(user) = from.user {
			let _ = config.user(&user);
		}
		if let Some(password) = from.password {
			let _ = config.password(password);
		}
		if let Some(dbname) = from.dbname {
			let _ = config.dbname(&dbname);
		}
		if let Some(options) = from.options {
			let _ = config.options(&options);
		}
		if let Some(connect_timeout) = from.connect_timeout {
			let _ = config.connect_timeout(connect_timeout);
		}
		config
	}
}
impl From<postgres::config::Config> for ConnectParams {
	fn from(from: postgres::config::Config) -> Self {
		Self {
			hosts: from.get_hosts().iter().cloned().map(Into::into).collect(),
			ports: from.get_ports().to_owned(),
			user: from.get_user().map(ToOwned::to_owned),
			password: from.get_password().map(ToOwned::to_owned),
			dbname: from.get_dbname().map(ToOwned::to_owned),
			options: from.get_options().map(ToOwned::to_owned),
			connect_timeout: from.get_connect_timeout().cloned(),
		}
	}
}
impl str::FromStr for ConnectParams {
	type Err = Box<dyn std::error::Error + 'static + Send + Sync>;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let params: postgres::config::Config = s.parse()?;
		Ok(params.into())
	}
}

#[derive(Serialize, Deserialize)]
enum Host {
	Tcp(String),
	Unix(PathBuf),
}
impl From<postgres::config::Host> for Host {
	fn from(from: postgres::config::Host) -> Self {
		match from {
			postgres::config::Host::Tcp(addr) => Host::Tcp(addr),
			postgres::config::Host::Unix(path) => Host::Unix(path),
		}
	}
}

pub struct Postgres<Row>
where
	Row: PostgresData,
{
	files: Vec<(ConnectParams, Vec<Source>)>,
	marker: PhantomData<fn() -> Row>,
}
impl<Row> Postgres<Row>
where
	Row: PostgresData,
{
	pub fn new<I>(files: I) -> Self
	where
		I: IntoIterator<Item = (ConnectParams, Vec<Source>)>,
	{
		Self {
			files: files.into_iter().collect(),
			marker: PhantomData,
		}
	}
}

impl<Row> DSource for Postgres<Row>
where
	Row: PostgresData,
{
	type Item = Row;
	type Error = PostgresError;

	#[cfg(not(feature = "doc"))]
	type DistStream = impl DistributedStream<Item = Result<Self::Item, Self::Error>>;
	#[cfg(feature = "doc")]
	type DistStream = amadeus_core::util::ImplDistributedStream<Result<Self::Item, Self::Error>>;

	#[allow(clippy::let_and_return)]
	fn dist_stream(self) -> Self::DistStream {
		let ret = self
			.files
			.into_dist_stream()
			.flat_map(FnMut!(|(config, tables)| async move {
				let (config, tables): (ConnectParams, Vec<Source>) = (config, tables);
				let (client, connection) = postgres::config::Config::from(config)
					.connect(postgres::tls::NoTls)
					.await
					.unwrap();
				tokio::spawn(async move {
					let _ = connection.await;
				});
				let client = Arc::new(client);
				stream::iter(tables.into_iter()).flat_map(move |table: Source| {
					let client = client.clone();
					async move {
						let table = match table {
							Source::Table(table) => table.to_string(),
							Source::Query(query) => format!("({}) _", query),
						};
						let query = format!(
							"COPY (SELECT {} FROM {}) TO STDOUT (FORMAT BINARY)",
							DisplayFmt::new(|f| Row::query(f, None)),
							table
						);
						let stmt = client.prepare(&query).await.unwrap();
						let stream = client.copy_out(&stmt).await.unwrap();
						BinaryCopyOutStream::new(stream)
							.map_ok(|row| {
								Row::decode(
									&postgres::types::Type::RECORD,
									row.as_ref().map(|bytes| bytes.as_ref()),
								)
								.unwrap()
							})
							.map_err(Into::into)
					}
					.flatten_stream()
				})
			}
			.flatten_stream()));
		#[cfg(feature = "doc")]
		let ret = amadeus_core::util::ImplDistributedStream::new(ret);
		ret
	}
}

#[pin_project]
/// A stream of rows deserialized from the PostgreSQL binary copy format.
pub struct BinaryCopyOutStream {
	#[pin]
	stream: CopyOutStream,
	header: bool,
}

impl BinaryCopyOutStream {
	/// Creates a stream from a raw copy out stream and the types of the columns being returned.
	pub fn new(stream: CopyOutStream) -> Self {
		BinaryCopyOutStream {
			stream,
			header: false,
		}
	}
}

impl Stream for BinaryCopyOutStream {
	type Item = Result<Option<Bytes>, PostgresError>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let self_ = self.project();

		let chunk = match ready!(self_.stream.poll_next(cx)) {
			Some(Ok(chunk)) => chunk,
			Some(Err(e)) => return Poll::Ready(Some(Err(e.into()))),
			None => {
				return Poll::Ready(Some(Err(io::Error::new(
					io::ErrorKind::UnexpectedEof,
					"connection closed",
				)
				.into())))
			}
		};
		let mut chunk = Cursor::new(chunk);

		if !*self_.header {
			check_remaining(&chunk, HEADER_LEN)?;
			if &chunk.bytes()[..MAGIC.len()] != MAGIC {
				return Poll::Ready(Some(Err(io::Error::new(
					io::ErrorKind::InvalidData,
					"error parsing response from server: invalid magic value",
				)
				.into())));
			}
			chunk.advance(MAGIC.len());

			let flags = chunk.get_i32();
			let has_oids = (flags & (1 << 16)) != 0;

			let header_extension = chunk.get_u32() as usize;
			check_remaining(&chunk, header_extension)?;
			chunk.advance(header_extension);

			assert!(!has_oids);
			*self_.header = true;
		}

		check_remaining(&chunk, 2)?;
		let len = chunk.get_i16();
		if len == -1 {
			return Poll::Ready(None);
		}

		assert_eq!(len, 1);

		check_remaining(&chunk, 4)?;
		let len = chunk.get_i32();
		if len == -1 {
			Poll::Ready(Some(Ok(None)))
		} else {
			let len = len as usize;
			check_remaining(&chunk, len)?;
			let start = chunk.position() as usize;
			Poll::Ready(Some(Ok(Some(chunk.into_inner().slice(start..start + len)))))
		}
	}
}

fn check_remaining(buf: &Cursor<Bytes>, len: usize) -> Result<(), PostgresError> {
	if buf.remaining() < len {
		Err(io::Error::new(
			io::ErrorKind::UnexpectedEof,
			"error parsing response from server: unexpected EOF",
		)
		.into())
	} else {
		Ok(())
	}
}

pub fn read_be_i32(buf: &mut &[u8]) -> io::Result<i32> {
	use std::io::Read;
	let mut bytes = [0; 4];
	buf.read_exact(&mut bytes)?;
	let num = ((i32::from(bytes[0])) << 24)
		| ((i32::from(bytes[1])) << 16)
		| ((i32::from(bytes[2])) << 8)
		| (i32::from(bytes[3]));
	Ok(num)
}

pub fn read_value<T>(
	type_: &::postgres::types::Type, buf: &mut &[u8],
) -> Result<T, Box<dyn std::error::Error + Sync + Send>>
where
	T: PostgresData,
{
	let len = read_be_i32(buf)?;
	let value = if len < 0 {
		None
	} else {
		let len = usize::try_from(len)?;
		if len > buf.len() {
			return Err(Into::into("invalid buffer size"));
		}
		let (head, tail) = buf.split_at(len);
		*buf = tail;
		Some(&head[..])
	};
	T::decode(type_, value)
}

// https://www.postgresql.org/docs/11/sql-syntax-lexical.html#SQL-SYNTAX-IDENTIFIERS
struct EscapeIdentifier<T>(T);
impl<T: Display> Display for EscapeIdentifier<T> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.write_str("\"")
			.and_then(|()| f.write_str(&self.0.to_string().replace('"', "\"\"")))
			.and_then(|()| f.write_str("\""))
	}
}

pub struct Names<'a>(pub Option<&'a Names<'a>>, pub &'static str);
impl<'a> Display for Names<'a> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		if let Some(prev) = self.0 {
			f.write_str("(")
				.and_then(|()| prev.fmt(f))
				.and_then(|()| f.write_str(")."))?;
		}
		EscapeIdentifier(self.1).fmt(f)
	}
}

// select column_name, is_nullable, data_type, character_maximum_length, * from information_schema.columns where table_name = 'weather' order by ordinal_position;
// select attname, atttypid, atttypmod, attnotnull, attndims from pg_attribute where attrelid = 'public.weather'::regclass and attnum > 0 and not attisdropped;

#[derive(Serialize, Deserialize, Debug)]
pub enum PostgresError {
	Io(IoError),
	Postgres(String),
}
impl PartialEq for PostgresError {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Io(a), Self::Io(b)) => a.to_string() == b.to_string(),
			(Self::Postgres(a), Self::Postgres(b)) => a == b,
			_ => false,
		}
	}
}
impl error::Error for PostgresError {}
impl Display for PostgresError {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::Io(err) => Display::fmt(err, f),
			Self::Postgres(err) => Display::fmt(err, f),
		}
	}
}
impl From<io::Error> for PostgresError {
	fn from(err: io::Error) -> Self {
		Self::Io(err.into())
	}
}
impl From<InternalPostgresError> for PostgresError {
	fn from(err: InternalPostgresError) -> Self {
		Self::Postgres(err.to_string())
	}
}

struct DisplayFmt<F>(F)
where
	F: Fn(&mut fmt::Formatter) -> fmt::Result;
impl<F> DisplayFmt<F>
where
	F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
	pub fn new(f: F) -> Self {
		Self(f)
	}
}
impl<F> Display for DisplayFmt<F>
where
	F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		self.0(f)
	}
}
