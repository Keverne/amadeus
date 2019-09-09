use constellation::*;
use serde_closure::FnMut;
use std::{
	env, path::PathBuf, time::{Duration, SystemTime}
};

use amadeus::{
	data::{
		types::{Downcast, Value}, Data
	}, source::{Csv, Source}, DistributedIterator, LocalPool, ProcessPool, ThreadPool
};

fn main() {
	init(Resources::default());

	// Accept the number of processes at the command line, defaulting to 10
	let processes = env::args()
		.nth(1)
		.and_then(|arg| arg.parse::<usize>().ok())
		.unwrap_or(10);

	let local_pool_time = {
		let local_pool = LocalPool::new();
		run(&local_pool)
	};
	let thread_pool_time = {
		let thread_pool = ThreadPool::new(processes).unwrap();
		run(&thread_pool)
	};
	let process_pool_time = {
		let process_pool = ProcessPool::new(processes, 1, Resources::default()).unwrap();
		run(&process_pool)
	};

	println!(
		"in {:?} {:?} {:?}",
		local_pool_time, thread_pool_time, process_pool_time
	);
}

fn run<P: amadeus_core::pool::ProcessPool>(pool: &P) -> Duration {
	let start = SystemTime::now();

	#[derive(Data, Clone, PartialEq, PartialOrd, Debug)]
	struct GameDerived {
		a: String,
		b: String,
		c: String,
		d: String,
		e: u32,
		f: String,
	}

	let rows =
		Csv::<_, GameDerived>::new(vec![PathBuf::from("amadeus-testing/csv/game.csv")]).unwrap();
	assert_eq!(
		rows.dist_iter()
			.map(FnMut!(|row: Result<_, _>| row.unwrap()))
			.count(pool),
		100_000
	);

	#[derive(Data, Clone, PartialEq, PartialOrd, Debug)]
	struct GameDerived2 {
		a: String,
		b: String,
		c: String,
		d: String,
		e: u64,
		f: String,
	}

	let rows = Csv::<_, Value>::new(vec![PathBuf::from("amadeus-testing/csv/game.csv")]).unwrap();
	assert_eq!(
		rows.dist_iter()
			.map(FnMut!(|row: Result<Value, _>| -> Value {
				let value = row.unwrap();
				// println!("{:?}", value);
				let _: GameDerived2 = value.clone().downcast().unwrap();
				value
			}))
			.count(pool),
		100_000
	);

	start.elapsed().unwrap()
}