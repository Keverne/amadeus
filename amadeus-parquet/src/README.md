# amadeus-parquet

[![Crates.io](https://img.shields.io/crates/v/amadeus-parquet.svg?maxAge=86400)](https://crates.io/crates/amadeus-parquet)
[![MIT / Apache 2.0 licensed](https://img.shields.io/crates/l/amadeus-parquet.svg?maxAge=2592000)](#License)
[![Build Status](https://dev.azure.com/alecmocatta/amadeus-parquet/_apis/build/status/tests?branchName=master)](https://dev.azure.com/alecmocatta/amadeus-parquet/_build/latest?branchName=master)

[Docs](https://docs.rs/amadeus-parquet/0.1.0)

An Apache Parquet implementation in Rust.

## Usage
Add this to your Cargo.toml:
```toml
[dependencies]
amadeus-parquet = "0.1.0"
```

Example usage of reading data untyped:
```rust
use std::fs::File;
use std::path::Path;
use amadeus_parquet::file::reader::{FileReader, SerializedFileReader};
use amadeus_parquet::record::types::Row;

let file = File::open(&Path::new("/path/to/file")).unwrap();
let reader = SerializedFileReader::new(file).unwrap();
let iter = reader.get_row_iter::<Row>().unwrap();
for record in iter.map(Result::unwrap) {
    println!("{:?}", record);
}
```

Example usage of reading data strongly-typed:
```rust
use std::fs::File;
use std::path::Path;
use amadeus_parquet::file::reader::{FileReader, SerializedFileReader};
use amadeus_parquet::record::{Record, types::Timestamp};

#[derive(Record, Debug)]
struct MyRow {
    id: u64,
    time: Timestamp,
    event: String,
}

let file = File::open(&Path::new("/path/to/file")).unwrap();
let reader = SerializedFileReader::new(file).unwrap();
let iter = reader.get_row_iter::<MyRow>(None).unwrap();
for record in iter.map(Result::unwrap) {
    println!("{}: {}", record.time, record.event);
}
```

## Supported Parquet Version
- Parquet-format 2.6.0

To update Parquet format to a newer version, check if [parquet-format](https://github.com/sunchao/parquet-format-rs)
version is available. Then simply update version of `parquet-format` crate in Cargo.toml.

## Features
- [X] All encodings supported
- [X] All compression codecs supported
- [X] Read support
  - [X] Primitive column value readers
  - [X] Row record reader
  - [ ] Arrow record reader
- [X] Statistics support
- [X] Write support
  - [X] Primitive column value writers
  - [ ] Row record writer
  - [ ] Arrow record writer
- [ ] Predicate pushdown
- [ ] Parquet format 2.5 support
- [ ] HDFS support

## Requirements
- Rust nightly

See [Working with nightly Rust](https://github.com/rust-lang-nursery/rustup.rs/blob/master/README.md#working-with-nightly-rust)
to install nightly toolchain and set it as default.

Parquet requires LLVM.  Our windows CI image includes LLVM but to build the libraries locally windows
users will have to install LLVM. Follow [this](https://github.com/appveyor/ci/issues/2651) link for info.

## Build
Run `cargo build` or `cargo build --release` to build in release mode.
Some features take advantage of SSE4.2 instructions, which can be
enabled by adding `RUSTFLAGS="-C target-feature=+sse4.2"` before the
`cargo build` command.

## Test
Run `cargo test` for unit tests.

## Binaries
The following binaries are provided (use `cargo install` to install them):
- **parquet-schema** for printing Parquet file schema and metadata.
`Usage: parquet-schema <file-path> [verbose]`, where `file-path` is the path to a Parquet file,
and optional `verbose` is the boolean flag that allows to print full metadata or schema only
(when not specified only schema will be printed).

- **parquet-read** for reading records from a Parquet file.
`Usage: parquet-read <file-path> [num-records]`, where `file-path` is the path to a Parquet file,
and `num-records` is the number of records to read from a file (when not specified all records will
be printed).

If you see `Library not loaded` error, please make sure `LD_LIBRARY_PATH` is set properly:
```
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:$(rustc --print sysroot)/lib
```

## Benchmarks
Run `cargo bench` for benchmarks.

## Docs
To build documentation, run `cargo doc --no-deps`.
To compile and view in the browser, run `cargo doc --no-deps --open`.

## License
Licensed under Apache License, Version 2.0, ([LICENSE.txt](LICENSE.txt) or
http://www.apache.org/licenses/LICENSE-2.0).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
licensed as above, without any additional terms or conditions.