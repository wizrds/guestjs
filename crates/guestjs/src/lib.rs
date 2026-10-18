#[allow(unused_extern_crates)]
extern crate self as guestjs;

pub mod prelude;

pub use guestjs_core::*;
pub use guestjs_macros::*;

#[cfg(feature = "quickjs")]
pub mod quickjs {
    pub use guestjs_quickjs::*;
}
