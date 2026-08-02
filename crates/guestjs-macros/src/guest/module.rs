use std::collections::HashMap;

use darling::{FromMeta, ast::NestedMeta};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, Ident, Meta, Pat, Path, ReturnType, Signature, Token, Type, Visibility,
    braced,
    parse::{Parse, ParseStream},
    spanned::Spanned,
};

use crate::{guest::GuestMacroError, path::CratePath};

mod keyword {
    syn::custom_keyword!(module);
    syn::custom_keyword!(value);
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ModuleOptions {
    crate_path: Option<Path>,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct MemberOptions {
    name: Option<String>,
}

struct GuestAttributes;

impl GuestAttributes {
    fn take<T>(attributes: &mut Vec<Attribute>) -> Result<T, GuestMacroError>
    where
        T: FromMeta,
    {
        let mut items = Vec::new();
        let mut retained = Vec::with_capacity(attributes.len());

        for attribute in attributes.drain(..) {
            if !attribute.path().is_ident("guestjs") {
                retained.push(attribute);

                continue;
            }

            match attribute.meta {
                Meta::Path(_) => {}
                Meta::List(list) => {
                    items.extend(NestedMeta::parse_meta_list(list.tokens)?);
                }
                meta @ Meta::NameValue(_) => {
                    return Err(syn::Error::new(
                        meta.span(),
                        "a guestjs attribute must use list syntax",
                    )
                    .into());
                }
            }
        }

        *attributes = retained;

        T::from_list(&items).map_err(Into::into)
    }
}

enum GuestMemberInput {
    Function(GuestFunctionInput),
    Value(GuestValueInput),
}

impl Parse for GuestMemberInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attributes = Attribute::parse_outer(input)?;

        if input.peek(keyword::value) {
            return Ok(Self::Value(GuestValueInput::parse(attributes, input)?));
        }

        Ok(Self::Function(GuestFunctionInput::parse(attributes, input)?))
    }
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

struct GuestFunctionInput {
    attributes: Vec<Attribute>,
    signature: Signature,
}

impl GuestFunctionInput {
    fn parse(attributes: Vec<Attribute>, input: ParseStream<'_>) -> syn::Result<Self> {
        Self { attributes, signature: input.parse()? }.with_semicolon(input)
    }

    fn with_semicolon(self, input: ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<Token![;]>()?;

        Ok(self)
    }
}

struct GuestValueInput {
    attributes: Vec<Attribute>,
    ident: Ident,
    descriptor: Type,
}

impl GuestValueInput {
    fn parse(attributes: Vec<Attribute>, input: ParseStream<'_>) -> syn::Result<Self> {
        input.parse::<keyword::value>()?;

        let ident = input.parse()?;

        input.parse::<Token![:]>()?;

        let descriptor = input.parse()?;

        input.parse::<Token![;]>()?;

        Ok(Self { attributes, ident, descriptor })
    }
}

struct GuestParameter {
    ident: Ident,
    descriptor: Type,
}

impl GuestParameter {
    fn new(argument: FnArg) -> Result<Self, GuestMacroError> {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new(
                argument.span(),
                "a guest module function cannot have a receiver",
            )
            .into());
        };

        if !argument.attrs.is_empty() {
            return Err(syn::Error::new(
                argument.span(),
                "guest module function parameters cannot have attributes",
            )
            .into());
        }

        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new(
                argument.pat.span(),
                "a guest module function parameter requires an identifier",
            )
            .into());
        };

        if pattern.by_ref.is_some() || pattern.mutability.is_some() || pattern.subpat.is_some() {
            return Err(syn::Error::new(
                pattern.span(),
                "a guest module function parameter requires a plain identifier",
            )
            .into());
        }

        Ok(Self {
            ident: pattern.ident.clone(),
            descriptor: argument.ty.as_ref().clone(),
        })
    }

    fn owned(&self, crate_path: &Path) -> TokenStream {
        let ident = &self.ident;
        let descriptor = &self.descriptor;

        quote!(
            #ident: <#descriptor as #crate_path::marshal::GuestType>::Owned
        )
    }

    fn bound(&self, crate_path: &Path) -> TokenStream {
        let ident = &self.ident;
        let descriptor = &self.descriptor;

        quote!(
            #ident: <#descriptor as #crate_path::marshal::GuestType>::Bound<'js>
        )
    }
}

trait GuestDescriptor {
    fn is_result(&self) -> bool;
}

impl GuestDescriptor for Type {
    fn is_result(&self) -> bool {
        matches!(
            self,
            Type::Path(path)
                if path.qself.is_none()
                    && path.path.segments.last().is_some_and(|segment| segment.ident == "Result")
        )
    }
}

struct GuestFunction {
    span: Span,
    attributes: Vec<Attribute>,
    ident: Ident,
    name: String,
    parameters: Vec<GuestParameter>,
    result: Type,
}

impl GuestFunction {
    fn new(mut input: GuestFunctionInput) -> Result<Self, GuestMacroError> {
        Self::validate_signature(&input.signature)?;

        let parameters = input
            .signature
            .inputs
            .iter()
            .cloned()
            .map(GuestParameter::new)
            .collect::<Result<Vec<_>, _>>()?;

        Self::validate_parameter_names(&parameters)?;

        Ok(Self {
            span: input.signature.ident.span(),
            name: GuestAttributes::take::<MemberOptions>(&mut input.attributes)?
                .name
                .unwrap_or_else(|| input.signature.ident.to_string()),
            attributes: input.attributes,
            result: Self::result_descriptor(&input.signature.output)?,
            ident: input.signature.ident,
            parameters,
        })
    }

    fn result_descriptor(output: &ReturnType) -> Result<Type, GuestMacroError> {
        match output {
            ReturnType::Type(_, result) if result.is_result() => Err(syn::Error::new(
                result.span(),
                "a guest module function names its successful descriptor, not Result",
            )
            .into()),
            ReturnType::Type(_, result) => Ok(result.as_ref().clone()),
            ReturnType::Default => Err(syn::Error::new(
                output.span(),
                "a guest module function requires a result descriptor",
            )
            .into()),
        }
    }

    fn validate_signature(signature: &Signature) -> Result<(), GuestMacroError> {
        if signature.constness.is_some()
            || signature.asyncness.is_some()
            || signature.unsafety.is_some()
            || signature.abi.is_some()
        {
            return Err(syn::Error::new(
                signature.span(),
                "a guest module function declaration cannot have qualifiers",
            )
            .into());
        }

        if !signature.generics.params.is_empty()
            || signature
                .generics
                .where_clause
                .is_some()
        {
            return Err(syn::Error::new(
                signature.generics.span(),
                "a guest module function declaration cannot be generic",
            )
            .into());
        }

        if signature.variadic.is_some() {
            return Err(syn::Error::new(
                signature.variadic.span(),
                "a guest module function declaration cannot be variadic",
            )
            .into());
        }

        if signature.inputs.len() > 4 {
            return Err(syn::Error::new(
                signature.inputs.span(),
                "a guest module function supports at most four parameters",
            )
            .into());
        }

        if signature.ident == "bind" {
            return Err(syn::Error::new(
                signature.ident.span(),
                "bind is reserved by the owned guest module facade",
            )
            .into());
        }

        Ok(())
    }

    fn validate_parameter_names(parameters: &[GuestParameter]) -> Result<(), GuestMacroError> {
        let mut names = HashMap::new();

        for parameter in parameters {
            let name = parameter.ident.to_string();

            if let Some(previous) = names.insert(name.clone(), parameter.ident.span()) {
                let mut error = syn::Error::new(
                    parameter.ident.span(),
                    format!("duplicate guest module function parameter {name:?}"),
                );

                error.combine(syn::Error::new(previous, "the first parameter is here"));

                return Err(error.into());
            }
        }

        Ok(())
    }

    fn arguments(&self) -> TokenStream {
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| &parameter.ident)
            .collect::<Vec<_>>();

        match parameters.as_slice() {
            [] => quote!(()),
            [parameter] => quote!((#parameter,)),
            _ => quote!((#(#parameters),*)),
        }
    }

    fn owned_method(&self, visibility: &Visibility, crate_path: &Path) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| parameter.owned(crate_path));
        let arguments = self.arguments();
        let result = &self.result;

        quote! {
            #(#attributes)*
            #[doc = concat!("Calls the `", #name, "` guest function.")]
            #visibility async fn #ident(
                &self
                #(, #parameters)*
            ) -> Result<
                <#result as #crate_path::marshal::FromGuest>::Owned,
                #crate_path::errors::Error,
            > {
                self.module
                    .function(#name)
                    .await?
                    .call::<_, #result>(#arguments)
                    .await
            }
        }
    }

    fn bound_method(&self, visibility: &Visibility, crate_path: &Path) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| parameter.bound(crate_path));
        let arguments = self.arguments();
        let result = &self.result;

        quote! {
            #(#attributes)*
            #[doc = concat!("Calls the `", #name, "` guest function.")]
            #visibility fn #ident(
                &self
                #(, #parameters)*
            ) -> Result<
                <#result as #crate_path::marshal::FromGuestBound>::Bound<'js>,
                #crate_path::errors::Error,
            > {
                self.module
                    .function(#name)?
                    .call::<_, #result>(#arguments)
            }
        }
    }
}

struct GuestValue {
    span: Span,
    attributes: Vec<Attribute>,
    ident: Ident,
    name: String,
    descriptor: Type,
}

impl GuestValue {
    fn new(mut input: GuestValueInput) -> Result<Self, GuestMacroError> {
        if input.ident == "bind" {
            return Err(syn::Error::new(
                input.ident.span(),
                "bind is reserved by the owned guest module facade",
            )
            .into());
        }

        if input.descriptor.is_result() {
            return Err(syn::Error::new(
                input.descriptor.span(),
                "a guest module value names its successful descriptor, not Result",
            )
            .into());
        }

        Ok(Self {
            span: input.ident.span(),
            name: GuestAttributes::take::<MemberOptions>(&mut input.attributes)?
                .name
                .unwrap_or_else(|| input.ident.to_string()),
            attributes: input.attributes,
            ident: input.ident,
            descriptor: input.descriptor,
        })
    }

    fn owned_method(&self, visibility: &Visibility, crate_path: &Path) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let descriptor = &self.descriptor;

        quote! {
            #(#attributes)*
            #[doc = concat!("Returns the `", #name, "` guest value.")]
            #visibility async fn #ident(
                &self,
            ) -> Result<
                <#descriptor as #crate_path::marshal::FromGuest>::Owned,
                #crate_path::errors::Error,
            > {
                self.module
                    .get::<#descriptor>(#name)
                    .await
            }
        }
    }

    fn bound_method(&self, visibility: &Visibility, crate_path: &Path) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let descriptor = &self.descriptor;

        quote! {
            #(#attributes)*
            #[doc = concat!("Returns the `", #name, "` guest value.")]
            #visibility fn #ident(
                &self,
            ) -> Result<
                <#descriptor as #crate_path::marshal::FromGuestBound>::Bound<'js>,
                #crate_path::errors::Error,
            > {
                self.module.get::<#descriptor>(#name)
            }
        }
    }
}

enum GuestMember {
    Function(GuestFunction),
    Value(GuestValue),
}

impl GuestMember {
    fn new(input: GuestMemberInput) -> Result<Self, GuestMacroError> {
        match input {
            GuestMemberInput::Function(function) => {
                Ok(Self::Function(GuestFunction::new(function)?))
            }
            GuestMemberInput::Value(value) => Ok(Self::Value(GuestValue::new(value)?)),
        }
    }

    fn span(&self) -> Span {
        match self {
            Self::Function(function) => function.span,
            Self::Value(value) => value.span,
        }
    }

    fn ident(&self) -> &Ident {
        match self {
            Self::Function(function) => &function.ident,
            Self::Value(value) => &value.ident,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Function(function) => &function.name,
            Self::Value(value) => &value.name,
        }
    }

    fn owned_method(&self, visibility: &Visibility, crate_path: &Path) -> TokenStream {
        match self {
            Self::Function(function) => function.owned_method(visibility, crate_path),
            Self::Value(value) => value.owned_method(visibility, crate_path),
        }
    }

    fn bound_method(&self, visibility: &Visibility, crate_path: &Path) -> TokenStream {
        match self {
            Self::Function(function) => function.bound_method(visibility, crate_path),
            Self::Value(value) => value.bound_method(visibility, crate_path),
        }
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
        let members = input
            .members
            .into_iter()
            .map(GuestMember::new)
            .collect::<Result<Vec<_>, _>>()?;
        let mut rust_names = HashMap::new();
        let mut guest_names = HashMap::new();

        for member in &members {
            Self::insert_name(
                &mut rust_names,
                member.ident().to_string(),
                member.span(),
                "Rust method",
            )?;
            Self::insert_name(&mut guest_names, member.name().to_owned(), member.span(), "export")?;
        }

        Ok(Self {
            crate_path: CratePath::new(
                GuestAttributes::take::<ModuleOptions>(&mut input.attributes)?.crate_path,
            )
            .resolve()?,
            attributes: input.attributes,
            visibility: input.visibility,
            bound_ident: format_ident!("Bound{}", input.ident),
            ident: input.ident,
            members,
        })
    }

    fn insert_name(
        names: &mut HashMap<String, Span>,
        name: String,
        span: Span,
        kind: &str,
    ) -> Result<(), GuestMacroError> {
        let Some(previous) = names.insert(name.clone(), span) else {
            return Ok(());
        };
        let mut error = syn::Error::new(span, format!("duplicate guest module {kind} {name:?}"));

        error.combine(syn::Error::new(previous, format!("the first {kind} is here")));

        Err(error.into())
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
            .map(|member| member.owned_method(&visibility, &crate_path));
        let bound_methods = members
            .iter()
            .map(|member| member.bound_method(&visibility, &crate_path));

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
