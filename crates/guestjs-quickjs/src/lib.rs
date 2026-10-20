#[allow(unused_extern_crates)]
extern crate self as guestjs_quickjs;

pub mod backend;
pub mod context;
pub mod engine;
pub mod error;
pub mod value;

pub use crate::{backend::QuickJs, engine::Config};
