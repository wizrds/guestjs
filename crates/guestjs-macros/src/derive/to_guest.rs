use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, GenericParam, LifetimeParam, parse_quote};

use crate::{derive::MarshalInput, path::CratePath};

pub(crate) struct ToGuestDerive {
    input: MarshalInput,
}

impl ToGuestDerive {
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
                #ident #type_generics: ::serde::Serialize
            ));

        let mut bound_generics = owned_generics.clone();

        bound_generics
            .params
            .insert(0, GenericParam::Lifetime(LifetimeParam::new(bound_lifetime.clone())));

        let (owned_impl_generics, _, owned_where_clause) = owned_generics.split_for_impl();
        let (bound_impl_generics, _, bound_where_clause) = bound_generics.split_for_impl();

        Ok(quote! {
            impl #owned_impl_generics #crate_path::marshal::ToGuest for #target
                #owned_where_clause
            {
                fn to_guest<'js>(
                    self,
                    scope: &#crate_path::runtime::Scope<'js>,
                ) -> Result<
                    #crate_path::__private::JsValue<'js>,
                    #crate_path::errors::Error,
                > {
                    #crate_path::__private::to_value(self, scope)
                }
            }

            impl #bound_impl_generics
                #crate_path::marshal::ToGuestBound<#bound_lifetime> for #target
                #bound_where_clause
            {
                fn to_guest_bound(
                    self,
                    scope: &#crate_path::runtime::Scope<#bound_lifetime>,
                ) -> Result<
                    #crate_path::__private::JsValue<#bound_lifetime>,
                    #crate_path::errors::Error,
                > {
                    #crate_path::__private::to_value(self, scope)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use crate::derive::to_guest::ToGuestDerive;

    #[test]
    fn accepts_structs_and_enums() {
        assert!(
            ToGuestDerive::new(&parse_quote!(
                struct Record;
            ))
            .is_ok()
        );
        assert!(
            ToGuestDerive::new(&parse_quote!(
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
            ToGuestDerive::new(&parse_quote!(union Value { integer: i32 }),)
                .err()
                .unwrap()
                .to_string()
                .contains("union"),
        );
    }

    #[test]
    fn preserves_generics_and_existing_bounds() {
        let output = ToGuestDerive::new(&parse_quote! {
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
        assert!(output.contains("Record < T , N > : :: serde :: Serialize",),);
    }

    #[test]
    fn uses_explicit_crate_path() {
        assert!(
            ToGuestDerive::new(&parse_quote! {
                #[guestjs(crate_path = custom::guestjs)]
                struct Record;
            },)
            .unwrap()
            .expand()
            .unwrap()
            .to_string()
            .contains("custom :: guestjs :: marshal :: ToGuest"),
        );
    }

    #[test]
    fn generates_owned_and_bound_implementations() {
        let output = ToGuestDerive::new(&parse_quote! {
            #[guestjs(crate_path = crate)]
            enum State {
                Ready,
            }
        })
        .unwrap()
        .expand()
        .unwrap()
        .to_string();

        assert!(output.contains("crate :: marshal :: ToGuest for State"));
        assert!(output.contains("crate :: marshal :: ToGuestBound < '__guestjs > for State",),);
        assert_eq!(
            output
                .matches("State : :: serde :: Serialize")
                .count(),
            2,
        );
    }

    #[test]
    fn avoids_existing_lifetime_names() {
        assert!(
            ToGuestDerive::new(&parse_quote! {
                #[guestjs(crate_path = crate)]
                struct Borrowed<'__guestjs> {
                    value: &'__guestjs str,
                }
            },)
            .unwrap()
            .expand()
            .unwrap()
            .to_string()
            .contains("ToGuestBound < '__guestjs1 >"),
        );
    }
}
