#[allow(unused_extern_crates)]
extern crate self as guestjs_macros;

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};
