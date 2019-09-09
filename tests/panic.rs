#![allow(where_clauses_object_safety)]

#[macro_use]
extern crate serde_closure;

use amadeus::prelude::*;
use constellation::*;
use std::panic;

fn main() {
	init(Resources::default());

	let pool = ProcessPool::new(3, 1, Resources::default()).unwrap();

	let res = panic::catch_unwind(|| {
		(0i32..1_000).into_dist_iter().for_each(
			&pool,
			FnMut!(|i| if i == 500 {
				panic!("boom")
			}),
		)
	});

	assert!(res.is_err());

	println!("done");
}