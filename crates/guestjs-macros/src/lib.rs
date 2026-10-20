#[allow(unused_extern_crates)]
extern crate self as guestjs_macros;

mod attributes;
mod backend;
mod bundle;
mod diagnostics;
mod guest;
mod host;
mod naming;
mod path;

#[cfg(feature = "serde")]
mod derive;

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};

use crate::{
    bundle::BundleMacro,
    guest::{class::GuestClassMacro, module::GuestModuleMacro},
    host::{class::HostClassMacro, exception::HostExceptionDerive, module::HostModuleMacro},
};

#[cfg(feature = "serde")]
use crate::derive::GuestDerive;

#[proc_macro]
pub fn bundle(input: TokenStream) -> TokenStream {
    match BundleMacro::new(input.into()) {
        Ok(bundle) => bundle.expand(),
        Err(error) => error.into_compile_error(),
    }
    .into()
}

#[proc_macro_attribute]
pub fn host_class(args: TokenStream, input: TokenStream) -> TokenStream {
    match HostClassMacro::new(args.into(), parse_macro_input!(input as ItemImpl)) {
        Ok(model) => model.expand(),
        Err(error) => error.into_tokens(),
    }
    .into()
}

#[proc_macro_attribute]
pub fn host_module(args: TokenStream, input: TokenStream) -> TokenStream {
    match HostModuleMacro::new(args.into(), parse_macro_input!(input as ItemImpl)) {
        Ok(model) => model.expand(),
        Err(error) => error.into_tokens(),
    }
    .into()
}

#[proc_macro]
pub fn guest_class(input: TokenStream) -> TokenStream {
    match GuestClassMacro::new(input.into()) {
        Ok(model) => model.expand(),
        Err(error) => error.into_tokens(),
    }
    .into()
}

#[proc_macro]
pub fn guest_module(input: TokenStream) -> TokenStream {
    match GuestModuleMacro::new(input.into()) {
        Ok(model) => model.expand(),
        Err(error) => error.into_tokens(),
    }
    .into()
}

#[proc_macro_derive(HostException, attributes(guestjs))]
pub fn derive_host_exception(input: TokenStream) -> TokenStream {
    match HostExceptionDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(model) => model.expand(),
        Err(error) => error.into_tokens(),
    }
    .into()
}

#[cfg(feature = "serde")]
#[proc_macro_derive(ToGuest, attributes(guestjs))]
pub fn derive_to_guest(input: TokenStream) -> TokenStream {
    match GuestDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(model) => model.to_guest(),
        Err(error) => error.into_tokens(),
    }
    .into()
}

#[cfg(feature = "serde")]
#[proc_macro_derive(FromGuest, attributes(guestjs))]
pub fn derive_from_guest(input: TokenStream) -> TokenStream {
    match GuestDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(model) => model.from_guest(),
        Err(error) => error.into_tokens(),
    }
    .into()
}
