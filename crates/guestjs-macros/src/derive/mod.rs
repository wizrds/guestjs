use darling::FromDeriveInput;
use proc_macro2::Span;
use syn::{DeriveInput, Generics, Ident, Lifetime, Path};

mod from_guest;
mod to_guest;

pub(crate) use crate::derive::{
    from_guest::FromGuestDerive,
    to_guest::ToGuestDerive,
};

#[derive(FromDeriveInput)]
#[darling(attributes(guestjs), supports(struct_any, enum_any))]
struct MarshalInput {
    ident: Ident,
    generics: Generics,
    crate_path: Option<Path>,
}

impl MarshalInput {
    fn new(input: &DeriveInput) -> Result<Self, darling::Error> {
        Self::from_derive_input(input)
    }

    fn bound_lifetime(&self) -> Lifetime {
        let mut suffix = 0;

        loop {
            let name = match suffix {
                0 => String::from("__guestjs"),
                _ => format!("__guestjs{suffix}"),
            };

            if !self.generics.lifetimes().any(|lifetime| {
                lifetime.lifetime.ident == name
            }) {
                return Lifetime::new(
                    &format!("'{name}"),
                    Span::mixed_site(),
                );
            }

            suffix += 1;
        }
    }
}
