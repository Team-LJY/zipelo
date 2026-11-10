#![allow(dead_code)]

#[path = "../../experiments/rust-core/src/budget.rs"]
mod budget;
#[path = "../../experiments/rust-typed-deflate/src/error.rs"]
mod error;
#[path = "../../experiments/rust-core/src/json_wire.rs"]
mod json_wire;
#[path = "../../experiments/rust-core/src/wire.rs"]
mod wire;
#[path = "../../experiments/rust-typed-deflate/src/zip_adapter.rs"]
mod zip_adapter;

#[path = "../rust-worker-unit/archive.rs"]
mod archive;
#[path = "../rust-worker-unit/bounds.rs"]
mod bounds;
#[path = "../rust-worker-unit/protocol.rs"]
mod protocol;
#[path = "../rust-worker-unit/support.rs"]
mod support;

#[path = "../rust-worker-unit/review.rs"]
mod review;

mod regression;
#[path = "../../experiments/rust-typed-deflate/src/typed_deflate.rs"]
mod typed_deflate;
