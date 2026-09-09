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
            GuestAttributes, GuestFacadeKind, GuestMembers, GuestMemberInput, keyword,
        },
    },
    path::CratePath,
};

#[derive(Default, darling::FromMeta)]
#[darling(default)]
struct ClassOptions {
    crate_path: Option<Path>,
    identity: Option<Path>,
}

struct GuestClassInput {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    ident: Ident,
    members: Vec<GuestMemberInput>,
}

impl Parse for GuestClassInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let content;
        let attributes = Attribute::parse_outer(input)?;
        let visibility = input.parse()?;

        input.parse::<keyword::class>()?;

        let ident = input.parse()?;

        braced!(content in input);

        let mut members = Vec::new();

        while !content.is_empty() {
            members.push(content.parse()?);
        }

        if !input.is_empty() {
            return Err(input.error("unexpected tokens after the guest class declaration"));
        }

        Ok(Self { attributes, visibility, ident, members })
    }
}

pub(crate) struct GuestClassMacro {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    ident: Ident,
    bound_ident: Ident,
    crate_path: Path,
    identity: Option<Path>,
    members: GuestMembers,
}

impl GuestClassMacro {
    pub(crate) fn new(input: TokenStream) -> Result<Self, GuestMacroError> {
        let mut input = syn::parse2::<GuestClassInput>(input)?;
        let members = GuestMembers::new(input.members, GuestFacadeKind::Class)?;
        let options = GuestAttributes::take::<ClassOptions>(&mut input.attributes)?;

        Ok(Self {
            crate_path: CratePath::new(options.crate_path).resolve()?,
            identity: options.identity,
            attributes: input.attributes,
            visibility: input.visibility,
            bound_ident: format_ident!("Bound{}", input.ident),
            ident: input.ident,
            members,
        })
    }

    fn owned_handle(&self) -> TokenStream {
        let crate_path = &self.crate_path;

        match &self.identity {
            Some(identity) => quote!(#crate_path::handle::Instance<#identity>),
            None => quote!(#crate_path::handle::Instance),
        }
    }

    fn bound_handle(&self) -> TokenStream {
        let crate_path = &self.crate_path;

        match &self.identity {
            Some(identity) => quote!(#crate_path::handle::BoundInstance<'js, #identity>),
            None => quote!(#crate_path::handle::BoundInstance<'js>),
        }
    }

    fn verification(&self) -> TokenStream {
        let crate_path = &self.crate_path;
        let Some(identity) = &self.identity else {
            return TokenStream::new();
        };

        quote! {
            ::std::mem::drop(
                <#identity as #crate_path::marshal::FromGuestRef>::from_guest_ref(
                    scope,
                    ::std::clone::Clone::clone(&value),
                )?,
            );
        }
    }

    pub(crate) fn expand(self) -> TokenStream {
        let owned_handle = self.owned_handle();
        let bound_handle = self.bound_handle();
        let verification = self.verification();
        let Self {
            attributes,
            visibility,
            ident,
            bound_ident,
            crate_path,
            members,
            ..
        } = self;
        let attributes = &attributes;
        let owned_methods = members
            .iter()
            .map(|member| member.owned_method(GuestFacadeKind::Class, &visibility, &crate_path));
        let bound_methods = members
            .iter()
            .map(|member| member.bound_method(GuestFacadeKind::Class, &visibility, &crate_path));

        quote! {
            #(#attributes)*
            #[doc = concat!(
                "An owned typed interface for the `",
                stringify!(#ident),
                "` guest class.",
            )]
            #visibility struct #ident {
                instance: #owned_handle,
            }

            impl #ident {
                /// Binds the instance to a scope.
                #visibility fn bind<'js>(
                    &self,
                    scope: &#crate_path::runtime::Scope<'js>,
                ) -> Result<#bound_ident<'js>, #crate_path::errors::Error> {
                    Ok(#bound_ident::from(self.instance.bind(scope)?))
                }

                /// Returns the underlying instance handle.
                #visibility fn instance(&self) -> &#owned_handle {
                    &self.instance
                }

                /// Converts the facade into the underlying instance handle.
                #visibility fn into_instance(self) -> #owned_handle {
                    self.instance
                }

                #(#owned_methods)*
            }

            impl ::std::convert::From<#owned_handle> for #ident {
                fn from(instance: #owned_handle) -> Self {
                    Self { instance }
                }
            }

            impl #crate_path::marshal::FromGuest for #ident {
                type Owned = Self;

                fn from_guest<'js>(
                    scope: &#crate_path::runtime::Scope<'js>,
                    value: #crate_path::value::JsValue<'js>,
                ) -> Result<Self::Owned, #crate_path::errors::Error> {
                    #verification

                    Ok(Self::from(
                        <#owned_handle as #crate_path::marshal::FromGuest>::from_guest(
                            scope,
                            value,
                        )?,
                    ))
                }
            }

            impl #crate_path::marshal::FromGuestBound for #ident {
                type Bound<'js> = #bound_ident<'js>;

                fn from_guest_bound<'js>(
                    scope: &#crate_path::runtime::Scope<'js>,
                    value: #crate_path::value::JsValue<'js>,
                ) -> Result<Self::Bound<'js>, #crate_path::errors::Error> {
                    #verification

                    Ok(#bound_ident::from(
                        <#owned_handle as #crate_path::marshal::FromGuestBound>
                            ::from_guest_bound(scope, value)?,
                    ))
                }
            }

            impl #crate_path::marshal::ToGuest for #ident {
                fn to_guest<'js>(
                    self,
                    scope: &#crate_path::runtime::Scope<'js>,
                ) -> Result<#crate_path::value::JsValue<'js>, #crate_path::errors::Error> {
                    #crate_path::marshal::ToGuest::to_guest(self.instance, scope)
                }
            }

            impl<'js> #crate_path::marshal::ToGuestBound<'js> for #ident {
                fn to_guest_bound(
                    self,
                    scope: &#crate_path::runtime::Scope<'js>,
                ) -> Result<#crate_path::value::JsValue<'js>, #crate_path::errors::Error> {
                    #crate_path::marshal::ToGuest::to_guest(self.instance, scope)
                }
            }

            #(#attributes)*
            #[doc = concat!(
                "A scope-bound typed interface for the `",
                stringify!(#ident),
                "` guest class.",
            )]
            #visibility struct #bound_ident<'js> {
                instance: #bound_handle,
            }

            impl<'js> #bound_ident<'js> {
                /// Returns the underlying instance handle.
                #visibility fn instance(&self) -> &#bound_handle {
                    &self.instance
                }

                /// Converts the facade into the underlying instance handle.
                #visibility fn into_instance(self) -> #bound_handle {
                    self.instance
                }

                /// Converts the facade into an owned handle.
                #visibility fn into_owned(
                    self,
                ) -> Result<#ident, #crate_path::errors::Error> {
                    Ok(#ident::from(self.instance.into_owned()?))
                }

                #(#bound_methods)*
            }

            impl<'js> ::std::convert::From<#bound_handle> for #bound_ident<'js> {
                fn from(instance: #bound_handle) -> Self {
                    Self { instance }
                }
            }

            impl<'js> #crate_path::marshal::ToGuestBound<'js> for #bound_ident<'js> {
                fn to_guest_bound(
                    self,
                    scope: &#crate_path::runtime::Scope<'js>,
                ) -> Result<#crate_path::value::JsValue<'js>, #crate_path::errors::Error> {
                    #crate_path::marshal::ToGuestBound::to_guest_bound(self.instance, scope)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use crate::guest::class::GuestClassMacro;

    #[test]
    fn generates_owned_and_bound_facades() {
        let output = GuestClassMacro::new(quote! {
            #[guestjs(crate_path = crate, identity = crate::Plugin)]
            pub class Greeter {
                fn ping() -> bool;

                #[guestjs(name = "handleRequest")]
                fn handle(
                    request: std::string::String,
                ) -> crate::handle::Promise<std::string::String>;

                value name: std::string::String;
                value config: crate::handle::Object;
            }
        })
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("pub struct Greeter"));
        assert!(output.contains("pub struct BoundGreeter < 'js >"));
        assert!(output.contains("An owned typed interface for the `"));
        assert!(output.contains("A scope-bound typed interface for the `"));
        assert!(output.contains("instance : crate :: handle :: Instance < crate :: Plugin >"));
        assert!(
            output
                .contains("instance : crate :: handle :: BoundInstance < 'js , crate :: Plugin >",)
        );
        assert!(output.contains("pub fn bind < 'js >"));
        assert!(output.contains("pub fn instance (& self)"));
        assert!(output.contains("pub fn into_instance (self)"));
        assert!(output.contains("pub async fn ping (& self)"));
        assert!(output.contains(concat!(
            "crate :: handle :: ObjectProtocol :: call_method :: < _ , bool > ",
            "(& self . instance , \"ping\" , ())",
        )));
        assert!(output.contains(concat!(
            "crate :: handle :: BoundObjectProtocol :: call_method :: < _ , ",
            "crate :: handle :: Promise",
        )));
        assert!(output.contains("(& self . instance , \"handleRequest\" , (request ,))"));
        assert!(output.contains(concat!(
            "crate :: handle :: ObjectProtocol :: get :: < std :: string :: String > ",
            "(& self . instance , \"name\")",
        )));
        assert!(output.contains(concat!(
            "crate :: handle :: BoundObjectProtocol :: get :: < crate :: handle :: Object > ",
            "(& self . instance , \"config\")",
        )));
        assert!(output.contains("impl crate :: marshal :: FromGuest for Greeter"));
        assert!(output.contains("impl crate :: marshal :: FromGuestBound for Greeter"));
        assert!(output.contains("type Bound < 'js > = BoundGreeter < 'js >"));
        assert!(output.contains("impl crate :: marshal :: ToGuest for Greeter"));
        assert!(output.contains("from_guest_ref"));
    }

    #[test]
    fn omits_the_identity_when_none_is_declared() {
        let output = GuestClassMacro::new(quote! {
            #[guestjs(crate_path = crate)]
            pub class Bare {
                fn ping() -> bool;
            }
        })
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains("instance : crate :: handle :: Instance ,"));
        assert!(output.contains("instance : crate :: handle :: BoundInstance < 'js >"));
        assert!(!output.contains("from_guest_ref"));
    }

    #[test]
    fn rejects_reserved_member_names() {
        for reserved in ["bind", "instance", "into_instance"] {
            let ident = quote::format_ident!("{reserved}");

            assert!(
                GuestClassMacro::new(quote! {
                    #[guestjs(crate_path = crate)]
                    pub class Reserved {
                        fn #ident() -> bool;
                    }
                })
                .is_err(),
            );
        }
    }
}
