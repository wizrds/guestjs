use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_quote};

use crate::{derive::MarshalInput, path::CratePath};

pub(crate) struct FromGuestDerive {
    input: MarshalInput,
}

impl FromGuestDerive {
    pub(crate) fn new(input: &DeriveInput) -> Result<Self, darling::Error> {
        Ok(Self { input: MarshalInput::new(input)? })
    }

    pub(crate) fn expand(self) -> Result<TokenStream, syn::Error> {
        let bound_lifetime = self.input.bound_lifetime();
        let MarshalInput { ident, generics, crate_path } = self.input;
        let crate_path = CratePath::new(crate_path).resolve()?;
        let (_, type_generics, _) = generics.split_for_impl();
        let target = quote!(#ident #type_generics);
        let mut owned_generics = generics.clone();

        owned_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #ident #type_generics: ::serde::de::DeserializeOwned
            ));
        owned_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #ident #type_generics: 'static
            ));

        let mut bound_generics = generics.clone();

        bound_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #ident #type_generics: ::serde::de::DeserializeOwned
            ));

        let (owned_impl_generics, _, owned_where_clause) = owned_generics.split_for_impl();
        let (bound_impl_generics, _, bound_where_clause) = bound_generics.split_for_impl();

        Ok(quote! {
            impl #owned_impl_generics #crate_path::marshal::FromGuest for #target
                #owned_where_clause
            {
                type Owned = Self;

                fn from_guest<'js>(
                    _scope: &#crate_path::runtime::Scope<'js>,
                    value: #crate_path::value::Value<'js>,
                ) -> Result<
                    Self::Owned,
                    #crate_path::errors::Error,
                > {
                    #crate_path::__private::from_value(value)
                }
            }

            impl #bound_impl_generics #crate_path::marshal::FromGuestBound for #target
                #bound_where_clause
            {
                type Bound<#bound_lifetime> = Self;

                fn from_guest_bound<#bound_lifetime>(
                    _scope: &#crate_path::runtime::Scope<#bound_lifetime>,
                    value: #crate_path::value::Value<#bound_lifetime>,
                ) -> Result<
                    Self::Bound<#bound_lifetime>,
                    #crate_path::errors::Error,
                > {
                    #crate_path::__private::from_value(value)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use crate::derive::from_guest::FromGuestDerive;

    #[test]
    fn accepts_structs_and_enums() {
        assert!(
            FromGuestDerive::new(&parse_quote!(
                struct Record;
            ))
            .is_ok()
        );
        assert!(
            FromGuestDerive::new(&parse_quote!(
                enum State {
                    Ready,
                }
            ))
            .is_ok()
        );
    }

    #[test]
    fn rejects_unions() {
        assert!(
            FromGuestDerive::new(&parse_quote!(union Value { integer: i32 }),)
                .err()
                .unwrap()
                .to_string()
                .contains("union"),
        );
    }

    #[test]
    fn preserves_generics_and_existing_bounds() {
        let output = FromGuestDerive::new(&parse_quote! {
            #[guestjs(crate_path = crate)]
            struct Record<T, const N: usize>
            where
                T: Clone,
            {
                values: [T; N],
            }
        })
        .unwrap()
        .expand()
        .unwrap()
        .to_string();

        assert!(output.contains("impl < T , const N : usize >"));
        assert!(output.contains("T : Clone"));
        assert_eq!(
            output
                .matches("Record < T , N > : :: serde :: de :: DeserializeOwned",)
                .count(),
            2,
        );
        assert!(output.contains("Record < T , N > : 'static"));
    }

    #[test]
    fn uses_explicit_crate_path() {
        assert!(
            FromGuestDerive::new(&parse_quote! {
                #[guestjs(crate_path = custom::guestjs)]
                struct Record;
            },)
            .unwrap()
            .expand()
            .unwrap()
            .to_string()
            .contains("custom :: guestjs :: marshal :: FromGuest"),
        );
    }

    #[test]
    fn generates_owned_and_bound_associated_types() {
        let output = FromGuestDerive::new(&parse_quote! {
            #[guestjs(crate_path = crate)]
            enum State {
                Ready,
            }
        })
        .unwrap()
        .expand()
        .unwrap()
        .to_string();

        assert!(output.contains("crate :: marshal :: FromGuest for State"));
        assert!(output.contains("type Owned = Self"));
        assert!(output.contains("crate :: marshal :: FromGuestBound for State"));
        assert!(output.contains("type Bound < '__guestjs > = Self"));
    }

    #[test]
    fn avoids_existing_lifetime_names() {
        assert!(
            FromGuestDerive::new(&parse_quote! {
                #[guestjs(crate_path = crate)]
                struct Borrowed<'__guestjs> {
                    value: &'__guestjs str,
                }
            },)
            .unwrap()
            .expand()
            .unwrap()
            .to_string()
            .contains("type Bound < '__guestjs1 >"),
        );
    }
}
