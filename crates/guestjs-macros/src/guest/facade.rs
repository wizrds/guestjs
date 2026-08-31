use std::collections::HashMap;

use darling::{FromMeta, ast::NestedMeta};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, Ident, Meta, Pat, Path, ReturnType, Signature, Token, Type, Visibility,
    parse::{Parse, ParseStream},
    spanned::Spanned,
};

use crate::guest::GuestMacroError;

pub(crate) mod keyword {
    syn::custom_keyword!(class);
    syn::custom_keyword!(module);
    syn::custom_keyword!(value);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuestFacadeKind {
    Class,
    Module,
}

impl GuestFacadeKind {
    fn noun(self) -> &'static str {
        match self {
            Self::Class => "guest class",
            Self::Module => "guest module",
        }
    }

    fn callable_noun(self) -> &'static str {
        match self {
            Self::Class => "method",
            Self::Module => "function",
        }
    }

    fn value_noun(self) -> &'static str {
        match self {
            Self::Class => "property",
            Self::Module => "value",
        }
    }

    fn reserved(self) -> &'static [&'static str] {
        match self {
            Self::Class => &["bind", "instance", "into_instance"],
            Self::Module => &["bind"],
        }
    }

    pub(crate) fn field(self) -> Ident {
        match self {
            Self::Class => format_ident!("instance"),
            Self::Module => format_ident!("module"),
        }
    }
}

#[derive(Default, FromMeta)]
#[darling(default)]
pub(crate) struct MemberOptions {
    pub(crate) name: Option<String>,
}

pub(crate) struct GuestAttributes;

impl GuestAttributes {
    pub(crate) fn take<T>(attributes: &mut Vec<Attribute>) -> Result<T, GuestMacroError>
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

pub(crate) struct GuestFunctionInput {
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

pub(crate) struct GuestValueInput {
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

pub(crate) enum GuestMemberInput {
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

struct GuestParameter {
    ident: Ident,
    descriptor: Type,
}

impl GuestParameter {
    fn new(argument: FnArg, kind: GuestFacadeKind) -> Result<Self, GuestMacroError> {
        let noun = kind.noun();
        let callable = kind.callable_noun();

        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new(
                argument.span(),
                format!("a {noun} {callable} cannot have a receiver"),
            )
            .into());
        };

        if !argument.attrs.is_empty() {
            return Err(syn::Error::new(
                argument.span(),
                format!("{noun} {callable} parameters cannot have attributes"),
            )
            .into());
        }

        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new(
                argument.pat.span(),
                format!("a {noun} {callable} parameter requires an identifier"),
            )
            .into());
        };

        if pattern.by_ref.is_some() || pattern.mutability.is_some() || pattern.subpat.is_some() {
            return Err(syn::Error::new(
                pattern.span(),
                format!("a {noun} {callable} parameter requires a plain identifier"),
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
                    && path.path.segments.last().is_some_and(|segment| {
                        segment.ident == "Result"
                    })
        )
    }
}

pub(crate) struct GuestFunction {
    span: Span,
    attributes: Vec<Attribute>,
    ident: Ident,
    name: String,
    parameters: Vec<GuestParameter>,
    result: Type,
}

impl GuestFunction {
    fn new(mut input: GuestFunctionInput, kind: GuestFacadeKind) -> Result<Self, GuestMacroError> {
        Self::validate_signature(&input.signature, kind)?;

        let parameters = input
            .signature
            .inputs
            .iter()
            .cloned()
            .map(|argument| GuestParameter::new(argument, kind))
            .collect::<Result<Vec<_>, _>>()?;

        Self::validate_parameter_names(&parameters, kind)?;

        Ok(Self {
            span: input.signature.ident.span(),
            name: GuestAttributes::take::<MemberOptions>(&mut input.attributes)?
                .name
                .unwrap_or_else(|| input.signature.ident.to_string()),
            attributes: input.attributes,
            result: Self::result_descriptor(&input.signature.output, kind)?,
            ident: input.signature.ident,
            parameters,
        })
    }

    fn result_descriptor(
        output: &ReturnType,
        kind: GuestFacadeKind,
    ) -> Result<Type, GuestMacroError> {
        let noun = kind.noun();
        let callable = kind.callable_noun();

        match output {
            ReturnType::Type(_, result) if result.is_result() => Err(syn::Error::new(
                result.span(),
                format!("a {noun} {callable} names its successful descriptor, not Result"),
            )
            .into()),
            ReturnType::Type(_, result) => Ok(result.as_ref().clone()),
            ReturnType::Default => Err(syn::Error::new(
                output.span(),
                format!("a {noun} {callable} requires a result descriptor"),
            )
            .into()),
        }
    }

    fn validate_signature(
        signature: &Signature,
        kind: GuestFacadeKind,
    ) -> Result<(), GuestMacroError> {
        let noun = kind.noun();
        let callable = kind.callable_noun();

        if signature.constness.is_some()
            || signature.asyncness.is_some()
            || signature.unsafety.is_some()
            || signature.abi.is_some()
        {
            return Err(syn::Error::new(
                signature.span(),
                format!("a {noun} {callable} declaration cannot have qualifiers"),
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
                format!("a {noun} {callable} declaration cannot be generic"),
            )
            .into());
        }

        if signature.variadic.is_some() {
            return Err(syn::Error::new(
                signature.variadic.span(),
                format!("a {noun} {callable} declaration cannot be variadic"),
            )
            .into());
        }

        if signature.inputs.len() > 4 {
            return Err(syn::Error::new(
                signature.inputs.span(),
                format!("a {noun} {callable} supports at most four parameters"),
            )
            .into());
        }

        Self::validate_ident(&signature.ident, kind)
    }

    fn validate_ident(ident: &Ident, kind: GuestFacadeKind) -> Result<(), GuestMacroError> {
        let name = ident.to_string();

        if !kind.reserved().contains(&name.as_str()) {
            return Ok(());
        }

        Err(syn::Error::new(
            ident.span(),
            format!("{name} is reserved by the owned {} facade", kind.noun()),
        )
        .into())
    }

    fn validate_parameter_names(
        parameters: &[GuestParameter],
        kind: GuestFacadeKind,
    ) -> Result<(), GuestMacroError> {
        let mut names = HashMap::new();

        for parameter in parameters {
            let name = parameter.ident.to_string();

            if let Some(previous) = names.insert(name.clone(), parameter.ident.span()) {
                let mut error = syn::Error::new(
                    parameter.ident.span(),
                    format!(
                        "duplicate {} {} parameter {name:?}",
                        kind.noun(),
                        kind.callable_noun(),
                    ),
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

    fn owned_body(&self, kind: GuestFacadeKind) -> TokenStream {
        let field = kind.field();
        let name = &self.name;
        let arguments = self.arguments();
        let result = &self.result;

        match kind {
            GuestFacadeKind::Class => quote! {
                self.#field
                    .call::<_, #result>(#name, #arguments)
                    .await
            },
            GuestFacadeKind::Module => quote! {
                self.#field
                    .function(#name)
                    .await?
                    .call::<_, #result>(#arguments)
                    .await
            },
        }
    }

    fn bound_body(&self, kind: GuestFacadeKind) -> TokenStream {
        let field = kind.field();
        let name = &self.name;
        let arguments = self.arguments();
        let result = &self.result;

        match kind {
            GuestFacadeKind::Class => quote! {
                self.#field.call::<_, #result>(#name, #arguments)
            },
            GuestFacadeKind::Module => quote! {
                self.#field
                    .function(#name)?
                    .call::<_, #result>(#arguments)
            },
        }
    }

    fn owned_method(
        &self,
        kind: GuestFacadeKind,
        visibility: &Visibility,
        crate_path: &Path,
    ) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let callable = kind.callable_noun();
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| parameter.owned(crate_path));
        let result = &self.result;
        let body = self.owned_body(kind);

        quote! {
            #(#attributes)*
            #[doc = concat!("Calls the `", #name, "` guest ", #callable, ".")]
            #visibility async fn #ident(
                &self
                #(, #parameters)*
            ) -> Result<
                <#result as #crate_path::marshal::FromGuest>::Owned,
                #crate_path::errors::Error,
            > {
                #body
            }
        }
    }

    fn bound_method(
        &self,
        kind: GuestFacadeKind,
        visibility: &Visibility,
        crate_path: &Path,
    ) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let callable = kind.callable_noun();
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| parameter.bound(crate_path));
        let result = &self.result;
        let body = self.bound_body(kind);

        quote! {
            #(#attributes)*
            #[doc = concat!("Calls the `", #name, "` guest ", #callable, ".")]
            #visibility fn #ident(
                &self
                #(, #parameters)*
            ) -> Result<
                <#result as #crate_path::marshal::FromGuestBound>::Bound<'js>,
                #crate_path::errors::Error,
            > {
                #body
            }
        }
    }
}

pub(crate) struct GuestValue {
    span: Span,
    attributes: Vec<Attribute>,
    ident: Ident,
    name: String,
    descriptor: Type,
}

impl GuestValue {
    fn new(mut input: GuestValueInput, kind: GuestFacadeKind) -> Result<Self, GuestMacroError> {
        GuestFunction::validate_ident(&input.ident, kind)?;

        if input.descriptor.is_result() {
            return Err(syn::Error::new(
                input.descriptor.span(),
                format!(
                    "a {} {} names its successful descriptor, not Result",
                    kind.noun(),
                    kind.value_noun(),
                ),
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

    fn owned_method(
        &self,
        kind: GuestFacadeKind,
        visibility: &Visibility,
        crate_path: &Path,
    ) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let value_noun = kind.value_noun();
        let descriptor = &self.descriptor;
        let field = kind.field();

        quote! {
            #(#attributes)*
            #[doc = concat!("Returns the `", #name, "` guest ", #value_noun, ".")]
            #visibility async fn #ident(
                &self,
            ) -> Result<
                <#descriptor as #crate_path::marshal::FromGuest>::Owned,
                #crate_path::errors::Error,
            > {
                self.#field
                    .get::<#descriptor>(#name)
                    .await
            }
        }
    }

    fn bound_method(
        &self,
        kind: GuestFacadeKind,
        visibility: &Visibility,
        crate_path: &Path,
    ) -> TokenStream {
        let attributes = &self.attributes;
        let ident = &self.ident;
        let name = &self.name;
        let value_noun = kind.value_noun();
        let descriptor = &self.descriptor;
        let field = kind.field();

        quote! {
            #(#attributes)*
            #[doc = concat!("Returns the `", #name, "` guest ", #value_noun, ".")]
            #visibility fn #ident(
                &self,
            ) -> Result<
                <#descriptor as #crate_path::marshal::FromGuestBound>::Bound<'js>,
                #crate_path::errors::Error,
            > {
                self.#field.get::<#descriptor>(#name)
            }
        }
    }
}

pub(crate) enum GuestMember {
    Function(GuestFunction),
    Value(GuestValue),
}

impl GuestMember {
    fn new(input: GuestMemberInput, kind: GuestFacadeKind) -> Result<Self, GuestMacroError> {
        match input {
            GuestMemberInput::Function(function) => {
                Ok(Self::Function(GuestFunction::new(function, kind)?))
            }
            GuestMemberInput::Value(value) => Ok(Self::Value(GuestValue::new(value, kind)?)),
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

    pub(crate) fn owned_method(
        &self,
        kind: GuestFacadeKind,
        visibility: &Visibility,
        crate_path: &Path,
    ) -> TokenStream {
        match self {
            Self::Function(function) => function.owned_method(kind, visibility, crate_path),
            Self::Value(value) => value.owned_method(kind, visibility, crate_path),
        }
    }

    pub(crate) fn bound_method(
        &self,
        kind: GuestFacadeKind,
        visibility: &Visibility,
        crate_path: &Path,
    ) -> TokenStream {
        match self {
            Self::Function(function) => function.bound_method(kind, visibility, crate_path),
            Self::Value(value) => value.bound_method(kind, visibility, crate_path),
        }
    }
}

pub(crate) struct GuestMembers;

impl GuestMembers {
    fn insert_name(
        names: &mut HashMap<String, Span>,
        name: String,
        span: Span,
        kind: GuestFacadeKind,
        role: &str,
    ) -> Result<(), GuestMacroError> {
        let Some(previous) = names.insert(name.clone(), span) else {
            return Ok(());
        };
        let mut error = syn::Error::new(span, format!("duplicate {} {role} {name:?}", kind.noun()));

        error.combine(syn::Error::new(previous, format!("the first {role} is here")));

        Err(error.into())
    }

    pub(crate) fn new(
        inputs: Vec<GuestMemberInput>,
        kind: GuestFacadeKind,
    ) -> Result<Vec<GuestMember>, GuestMacroError> {
        let members = inputs
            .into_iter()
            .map(|input| GuestMember::new(input, kind))
            .collect::<Result<Vec<_>, _>>()?;
        let mut rust_names = HashMap::new();
        let mut guest_names = HashMap::new();

        for member in &members {
            Self::insert_name(
                &mut rust_names,
                member.ident().to_string(),
                member.span(),
                kind,
                "Rust method",
            )?;
            Self::insert_name(
                &mut guest_names,
                member.name().to_owned(),
                member.span(),
                kind,
                "export",
            )?;
        }

        Ok(members)
    }
}
