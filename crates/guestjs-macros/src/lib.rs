#[allow(unused_extern_crates)]
extern crate self as guestjs_macros;

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};

mod derive;
mod guest;
mod host;
mod path;

use crate::{
    derive::{FromGuestDerive, ToGuestDerive},
    guest::GuestModuleMacro,
    host::{HostClassMacro, HostModuleMacro},
};

/// Derives conversion from a guest value into a Rust value.
#[proc_macro_derive(FromGuest, attributes(guestjs))]
pub fn derive_from_guest(input: TokenStream) -> TokenStream {
    match FromGuestDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(derive) => derive
            .expand()
            .unwrap_or_else(syn::Error::into_compile_error),
        Err(error) => error.write_errors(),
    }
    .into()
}

/// Derives conversion from a Rust value into a guest value.
#[proc_macro_derive(ToGuest, attributes(guestjs))]
pub fn derive_to_guest(input: TokenStream) -> TokenStream {
    match ToGuestDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(derive) => derive
            .expand()
            .unwrap_or_else(syn::Error::into_compile_error),
        Err(error) => error.write_errors(),
    }
    .into()
}

/// Defines a host class.
#[proc_macro_attribute]
pub fn host_class(args: TokenStream, input: TokenStream) -> TokenStream {
    match HostClassMacro::new(args.into(), parse_macro_input!(input as ItemImpl)) {
        Ok(class) => class.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}

/// Defines a host module.
#[proc_macro_attribute]
pub fn host_module(args: TokenStream, input: TokenStream) -> TokenStream {
    match HostModuleMacro::new(args.into(), parse_macro_input!(input as ItemImpl)) {
        Ok(module) => module.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}

/// Defines typed access to a guest module.
#[proc_macro]
pub fn guest_module(input: TokenStream) -> TokenStream {
    match GuestModuleMacro::new(input.into()) {
        Ok(module) => module.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}
