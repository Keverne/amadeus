use futures::FutureExt;
use std::{panic, panic::AssertUnwindSafe, time::SystemTime};

use amadeus::prelude::*;

#[tokio::test]
async fn panic() {
	let start = SystemTime::now();

	let pool = &ThreadPool::new(None).unwrap();

	let res = AssertUnwindSafe((0i32..1_000).into_par_stream().for_each(pool, |i| {
		if i == 500 {
			panic!("this is intended to panic")
		}
	}))
	.catch_unwind()
	.await;

	assert!(res.is_err());

	println!("in {:?}", start.elapsed().unwrap());
}
