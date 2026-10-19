#[allow(unused_extern_crates)]
extern crate self as guestjs_macros;

mod bundle;
mod path;

use proc_macro::TokenStream;

use crate::bundle::BundleMacro;

#[proc_macro]
pub fn bundle(input: TokenStream) -> TokenStream {
    match BundleMacro::new(input.into()) {
        Ok(bundle) => bundle.expand(),
        Err(error) => error.into_compile_error(),
    }
    .into()
}
