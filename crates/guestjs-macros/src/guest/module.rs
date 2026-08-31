use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Ident, Path, Visibility, braced,
    parse::{Parse, ParseStream},
};

use crate::{
    guest::{
        GuestMacroError,
        facade::{
            GuestAttributes, GuestFacadeKind, GuestMember, GuestMemberInput, GuestMembers, keyword,
        },
    },
    path::CratePath,
};

#[derive(Default, darling::FromMeta)]
#[darling(default)]
struct ModuleOptions {
    crate_path: Option<Path>,
}

struct GuestModuleInput {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    ident: Ident,
    members: Vec<GuestMemberInput>,
}

impl Parse for GuestModuleInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let content;
        let attributes = Attribute::parse_outer(input)?;
        let visibility = input.parse()?;

        input.parse::<keyword::module>()?;

        let ident = input.parse()?;

        braced!(content in input);

        let mut members = Vec::new();

        while !content.is_empty() {
            members.push(content.parse()?);
        }

        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the guest module declaration"));
        }

        Ok(Self { attributes, visibility, ident, members })
    }
}

pub(crate) struct GuestModuleMacro {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    ident: Ident,
    bound_ident: Ident,
    crate_path: Path,
    members: Vec<GuestMember>,
}

impl GuestModuleMacro {
    pub(crate) fn new(input: TokenStream) -> Result<Self, GuestMacroError> {
        let mut input = syn::parse2::<GuestModuleInput>(input)?;

        Ok(Self {
            members: GuestMembers::new(input.members, GuestFacadeKind::Module)?,
            crate_path: CratePath::new(
                GuestAttributes::take::<ModuleOptions>(&mut input.attributes)?.crate_path,
            )
            .resolve()?,
            attributes: input.attributes,
            visibility: input.visibility,
            bound_ident: format_ident!("Bound{}", input.ident),
            ident: input.ident,
        })
    }

    pub(crate) fn expand(self) -> TokenStream {
        let Self {
            attributes,
            visibility,
            ident,
            bound_ident,
            crate_path,
            members,
        } = self;
        let attributes = &attributes;
        let owned_methods = members
            .iter()
            .map(|member| member.owned_method(GuestFacadeKind::Module, &visibility, &crate_path));
        let bound_methods = members
            .iter()
            .map(|member| member.bound_method(GuestFacadeKind::Module, &visibility, &crate_path));

        quote! {
            #(#attributes)*
            #[doc = concat!(
                "An owned typed interface for the `",
                stringify!(#ident),
                "` guest module.",
            )]
            #visibility struct #ident {
                module: #crate_path::handle::Module,
            }

            impl #ident {
                /// Binds the module to a scope.
                #visibility fn bind<'js>(
                    &self,
                    scope: &#crate_path::runtime::Scope<'js>,
                ) -> Result<#bound_ident<'js>, #crate_path::errors::Error> {
                    Ok(#bound_ident::from(self.module.bind(scope)?))
                }

                #(#owned_methods)*
            }

            impl ::std::convert::From<#crate_path::handle::Module> for #ident {
                fn from(module: #crate_path::handle::Module) -> Self {
                    Self { module }
                }
            }

            #(#attributes)*
            #[doc = concat!(
                "A scope-bound typed interface for the `",
                stringify!(#ident),
                "` guest module.",
            )]
            #visibility struct #bound_ident<'js> {
                module: #crate_path::handle::BoundModule<'js>,
            }

            impl<'js> #bound_ident<'js> {
                #(#bound_methods)*
            }

            impl<'js> ::std::convert::From<#crate_path::handle::BoundModule<'js>>
                for #bound_ident<'js>
            {
                fn from(module: #crate_path::handle::BoundModule<'js>) -> Self {
                    Self { module }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use crate::guest::module::GuestModuleMacro;

    #[test]
    fn generates_owned_and_bound_facades() {
        let output = GuestModuleMacro::new(quote! {
            #[guestjs(crate_path = crate)]
            pub module Math {
                fn ping() -> bool;

                fn apply(
                    callback: crate::handle::Function,
                ) -> i32;

                #[guestjs(name = "combine")]
                fn add(
                    left: std::option::Option<i32>,
                    right: crate::marshal::Nullish<i32>,
                ) -> crate::handle::Promise<i32>;

                value answer: i32;
                value optional: std::option::Option<i32>;
                value nullish: crate::marshal::Nullish<i32>;
                value settings: crate::handle::Object;
                value counter: crate::handle::Class;

                #[guestjs(name = "operation")]
                value callback: crate::handle::Function;

                value pending: crate::handle::Promise<crate::handle::Function>;
            }
        })
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("pub struct Math"));
        assert!(output.contains("pub struct BoundMath < 'js >"));
        assert!(output.contains("An owned typed interface for the `"));
        assert!(output.contains("A scope-bound typed interface for the `"));
        assert!(output.contains("From < crate :: handle :: Module > for Math"));
        assert!(output.contains("From < crate :: handle :: BoundModule"));
        assert!(output.contains("for BoundMath < 'js >"));
        assert!(output.contains("pub fn bind < 'js >"));
        assert!(output.contains("pub async fn ping (& self)"));
        assert!(output.contains("call :: < _ , bool > (())"));
        assert!(output.contains(
            "callback : < crate :: handle :: Function as crate :: marshal :: GuestType > :: Owned",
        ));
        assert!(output.contains(concat!(
            "callback : < crate :: handle :: Function as ",
            "crate :: marshal :: GuestType > :: Bound < 'js >",
        ),));
        assert!(output.contains("call :: < _ , i32 > ((callback ,))"));
        assert!(output.contains("function (\"combine\")"));
        assert!(output.contains(
            "< crate :: handle :: Promise < i32 > as crate :: marshal :: FromGuest > :: Owned",
        ));
        assert!(output.contains(concat!(
            "< crate :: handle :: Promise < i32 > as ",
            "crate :: marshal :: FromGuestBound > :: Bound < 'js >",
        ),));
        assert!(output.contains("call :: < _ , crate :: handle :: Promise < i32 > >"));
        assert!(output.contains("pub async fn answer"));
        assert!(output.contains("pub fn answer"));
        assert!(output.contains("get :: < i32 > (\"answer\")"));
        assert!(output.contains("get :: < crate :: handle :: Object > (\"settings\")"));
        assert!(output.contains("get :: < crate :: handle :: Class > (\"counter\")"));
        assert!(output.contains("get :: < crate :: handle :: Function > (\"operation\")"));
        assert!(output.contains("get :: < crate :: handle :: Promise"));
        assert!(output.contains("(\"pending\")"));
        assert!(
            output.contains(
                "< crate :: handle :: Object as crate :: marshal :: FromGuest > :: Owned",
            )
        );
        assert!(output.contains(concat!(
            "< crate :: handle :: Object as crate :: marshal :: FromGuestBound > :: ",
            "Bound < 'js >",
        ),));
    }

    #[test]
    fn preserves_visibility_and_explicit_crate_path() {
        let output = GuestModuleMacro::new(quote! {
            #[guestjs(crate_path = custom::guestjs)]
            #[allow(dead_code)]
            pub(crate) module Internal {
                #[allow(clippy::needless_lifetimes)]
                fn read(value: i32) -> i32;
            }
        })
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("pub (crate) struct Internal"));
        assert!(output.contains("pub (crate) struct BoundInternal"));
        assert!(output.contains("custom :: guestjs :: handle :: Module"));
        assert!(output.contains("pub (crate) async fn read"));
        assert!(output.contains("pub (crate) fn read"));
        assert_eq!(
            output
                .matches("allow (dead_code)")
                .count(),
            2
        );
        assert_eq!(
            output
                .matches("allow (clippy :: needless_lifetimes)")
                .count(),
            2
        );
    }

    #[test]
    fn rejects_invalid_guest_module_declarations() {
        let cases = [
            quote! {
                #[guestjs(crate_path = crate)]
                module DuplicateRust {
                    fn read() -> i32;
                    fn read(value: i32) -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module DuplicateGuest {
                    #[guestjs(name = "read")]
                    fn first() -> i32;

                    fn read() -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module Reserved {
                    fn bind() -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module Qualified {
                    async fn read() -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module MissingResult {
                    fn read();
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module ExplicitResult {
                    fn read() -> Result<i32, crate::errors::Error>;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module TooMany {
                    fn read(
                        first: i32,
                        second: i32,
                        third: i32,
                        fourth: i32,
                        fifth: i32,
                    ) -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module Receiver {
                    fn read(&self) -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module Pattern {
                    fn read(mut value: i32) -> i32;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module DuplicateKinds {
                    fn version() -> String;
                    value version: String;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module DuplicateGuestKinds {
                    #[guestjs(name = "version")]
                    fn read() -> String;

                    value version: String;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module ReservedValue {
                    value bind: String;
                }
            },
            quote! {
                #[guestjs(crate_path = crate)]
                module ExplicitValueResult {
                    value version: Result<String, crate::errors::Error>;
                }
            },
        ];

        for case in cases {
            assert!(GuestModuleMacro::new(case).is_err());
        }
    }
}
