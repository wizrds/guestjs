use darling::{FromMeta, util::Flag};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Generics, Ident, Path, Type, Variant, parse_quote};

use crate::{
    attributes::HelperAttributes,
    backend::{BackendBounds, BackendOption, BackendParameter},
    diagnostics::MacroError,
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct DeriveOptions {
    crate_path: Option<Path>,
    backend: Option<BackendOption>,
    #[darling(rename = "union")]
    untagged: Flag,
}

impl DeriveOptions {
    fn into_parts(self) -> (Option<Path>, Option<BackendOption>, bool) {
        (self.crate_path, self.backend, self.untagged.is_present())
    }
}

struct UnionVariant {
    ident: Ident,
    ty: Type,
}

impl UnionVariant {
    fn new(variant: &Variant) -> syn::Result<Self> {
        let Fields::Unnamed(fields) = &variant.fields else {
            return Err(syn::Error::new_spanned(
                variant,
                "a union variant requires exactly one unnamed field",
            ));
        };

        if fields.unnamed.len() != 1 {
            return Err(syn::Error::new_spanned(
                variant,
                "a union variant requires exactly one unnamed field",
            ));
        }

        Ok(Self {
            ident: variant.ident.clone(),
            ty: fields.unnamed[0].ty.clone(),
        })
    }

    fn ty(&self) -> &Type {
        &self.ty
    }

    fn render_to_guest(&self, krate: &Path, backend: &Type) -> TokenStream {
        let Self { ident, ty } = self;

        quote! {
            Self::#ident(value) => {
                <#ty as #krate::marshal::ToGuest<#backend>>::to_guest(
                    value,
                    enter,
                )
            }
        }
    }

    fn render_from_guest(&self, krate: &Path, backend: &Type) -> TokenStream {
        let Self { ident, ty } = self;

        quote! {
            match <#ty as #krate::marshal::FromGuest<#backend>>::from_guest(
                enter,
                ::core::clone::Clone::clone(&value),
            ) {
                ::core::result::Result::Ok(value) => {
                    return ::core::result::Result::Ok(Self::#ident(value));
                }
                ::core::result::Result::Err(error)
                    if error.is_conversion() => {}
                ::core::result::Result::Err(error) => {
                    return ::core::result::Result::Err(error);
                }
            }
        }
    }
}

struct UnionDerive {
    variants: Vec<UnionVariant>,
}

impl UnionDerive {
    fn new(input: &DeriveInput) -> syn::Result<Self> {
        let Data::Enum(data) = &input.data else {
            return Err(syn::Error::new_spanned(
                input,
                "union requires a nonempty enum of single-field variants",
            ));
        };

        if data.variants.is_empty() {
            return Err(syn::Error::new_spanned(input, "union cannot be empty"));
        }

        Ok(Self {
            variants: data
                .variants
                .iter()
                .map(UnionVariant::new)
                .collect::<Result<_, _>>()?,
        })
    }

    fn field_types(&self) -> impl Iterator<Item = &Type> {
        self.variants
            .iter()
            .map(UnionVariant::ty)
    }

    fn render_to_guest(&self, krate: &Path, backend: &Type) -> TokenStream {
        let arms = self
            .variants
            .iter()
            .map(|variant| variant.render_to_guest(krate, backend));

        quote!(match self { #(#arms),* })
    }

    fn render_from_guest(&self, krate: &Path, backend: &Type) -> TokenStream {
        let attempts = self
            .variants
            .iter()
            .map(|variant| variant.render_from_guest(krate, backend));

        quote! {
            #(#attempts)*

            ::core::result::Result::Err(
                #krate::marshal::describe::Expected::of::<Self>()
                    .mismatch(
                        <#backend as #krate::backend::values::BackendValues>::kind(
                            enter.token(),
                            &value,
                        ),
                    ),
            )
        }
    }

    fn render_describe(&self, krate: &Path, name: &Ident, source: &Generics) -> TokenStream {
        let mut generics = source.clone();

        for ty in self.field_types() {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #krate::marshal::describe::Describe));
        }

        let (implementation, target, clause) = generics.split_for_impl();
        let fields = self.field_types();

        quote! {
            impl #implementation #krate::marshal::describe::Describe
                for #name #target #clause
            {
                fn describe(
                    expected: &mut #krate::marshal::describe::Expected,
                ) {
                    #(
                        <#fields as #krate::marshal::describe::Describe>::describe(
                            expected,
                        );
                    )*
                }
            }
        }
    }
}

enum DeriveShape {
    Serde,
    Union(UnionDerive),
}

pub struct GuestDerive {
    name: Ident,
    generics: Generics,
    krate: Path,
    backend: BackendParameter,
    shape: DeriveShape,
}

impl GuestDerive {
    pub fn new(input: &DeriveInput) -> Result<Self, MacroError> {
        let (path, backend, union) =
            DeriveOptions::from_list(&HelperAttributes::take(&mut input.attrs.clone())?)?
                .into_parts();

        if matches!(input.data, Data::Union(_)) {
            return Err(syn::Error::new_spanned(input, "Rust unions are not supported").into());
        }

        Ok(Self {
            name: input.ident.clone(),
            generics: input.generics.clone(),
            krate: CratePath::new(path).resolve()?,
            backend: BackendParameter::resolve(backend, &input.generics, &[])?,
            shape: if union {
                DeriveShape::Union(UnionDerive::new(input)?)
            } else {
                DeriveShape::Serde
            },
        })
    }

    fn render(&self, from: bool) -> TokenStream {
        let krate = &self.krate;
        let name = &self.name;
        let backend = self.backend.ty();
        let (_, target_generics, _) = self.generics.split_for_impl();
        let target: Type = parse_quote!(#name #target_generics);
        let mut generics = self.backend.definition_generics(
            &self.generics,
            &BackendBounds::new(
                &self.backend,
                vec![parse_quote!(#krate::backend::values::BackendValues)],
            ),
        );

        if from {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#target: 'static));
        }

        let (body, description) = match &self.shape {
            DeriveShape::Serde => {
                if from {
                    generics
                        .make_where_clause()
                        .predicates
                        .push(parse_quote!(
                            #krate::marshal::serde::Serde<#target>:
                                #krate::marshal::FromGuest<
                                    #backend,
                                    Owned = #target,
                                >
                        ));

                    (
                        quote! {
                            <#krate::marshal::serde::Serde<Self> as
                                #krate::marshal::FromGuest<#backend>>::from_guest(
                                    enter,
                                    value,
                                )
                        },
                        TokenStream::new(),
                    )
                } else {
                    generics
                        .make_where_clause()
                        .predicates
                        .push(parse_quote!(
                            #krate::marshal::serde::Serde<#target>:
                                #krate::marshal::ToGuest<#backend>
                        ));

                    (
                        quote! {
                            #krate::marshal::ToGuest::to_guest(
                                #krate::marshal::serde::Serde::new(self),
                                enter,
                            )
                        },
                        TokenStream::new(),
                    )
                }
            }
            DeriveShape::Union(union) => {
                for ty in union.field_types() {
                    generics
                        .make_where_clause()
                        .predicates
                        .push(if from {
                            parse_quote!(
                                #ty: #krate::marshal::FromGuest<
                                    #backend,
                                    Owned = #ty,
                                > + #krate::marshal::describe::Describe
                            )
                        } else {
                            parse_quote!(#ty: #krate::marshal::ToGuest<#backend>)
                        });
                }

                if from {
                    (
                        union.render_from_guest(krate, &backend),
                        union.render_describe(krate, name, &self.generics),
                    )
                } else {
                    (union.render_to_guest(krate, &backend), TokenStream::new())
                }
            }
        };
        let (implementation, _, clause) = generics.split_for_impl();

        if from {
            quote! {
                #description

                impl #implementation #krate::marshal::FromGuest<#backend>
                    for #target #clause
                {
                    type Owned = Self;

                    fn from_guest<'js>(
                        enter: &#krate::enter::Enter<'js, #backend>,
                        value: #krate::backend::Val<'js, #backend>,
                    ) -> ::core::result::Result<
                        Self::Owned,
                        #krate::errors::Error,
                    > {
                        #body
                    }
                }
            }
        } else {
            quote! {
                impl #implementation #krate::marshal::ToGuest<#backend>
                    for #target #clause
                {
                    fn to_guest<'js>(
                        self,
                        enter: &#krate::enter::Enter<'js, #backend>,
                    ) -> ::core::result::Result<
                        #krate::backend::Val<'js, #backend>,
                        #krate::errors::Error,
                    > {
                        #body
                    }
                }
            }
        }
    }

    pub fn to_guest(&self) -> TokenStream {
        self.render(false)
    }

    pub fn from_guest(&self) -> TokenStream {
        self.render(true)
    }
}

#[cfg(test)]
mod tests {
    use syn::{Expr, File, parse_quote, parse2};

    use crate::derive::{GuestDerive, UnionVariant};

    #[test]
    fn emits_describe_only_with_from_guest() {
        let derive = GuestDerive::new(&parse_quote!(
            #[guestjs(union, crate_path = crate)]
            enum NameOrCount {
                Name(String),
                Count(f64),
            }
        ))
        .unwrap_or_else(|error| panic!("{}", error.into_tokens()));

        assert_eq!(
            parse2::<File>(derive.from_guest())
                .unwrap()
                .items
                .len(),
            2,
        );
        assert_eq!(
            parse2::<File>(derive.to_guest())
                .unwrap()
                .items
                .len(),
            1,
        );
    }

    #[test]
    fn rejects_invalid_union_shapes() {
        for input in [
            parse_quote!(
                #[guestjs(union, crate_path = crate)]
                enum Empty {}
            ),
            parse_quote!(
                #[guestjs(union, crate_path = crate)]
                enum Named {
                    Value { value: String },
                }
            ),
            parse_quote!(
                #[guestjs(union, crate_path = crate)]
                enum Many {
                    Value(String, f64),
                }
            ),
        ] {
            assert!(GuestDerive::new(&input).is_err());
        }
    }

    #[test]
    fn union_attempt_propagates_operational_errors() {
        assert_eq!(
            parse2::<Expr>(
                UnionVariant::new(&parse_quote!(Name(String)))
                    .unwrap()
                    .render_from_guest(&parse_quote!(crate), &parse_quote!(B),),
            )
            .unwrap(),
            parse_quote!(match <String as crate::marshal::FromGuest<B>>::from_guest(
                enter,
                ::core::clone::Clone::clone(&value),
            ) {
                ::core::result::Result::Ok(value) => {
                    return ::core::result::Result::Ok(Self::Name(value));
                }
                ::core::result::Result::Err(error) if error.is_conversion() => {}
                ::core::result::Result::Err(error) => {
                    return ::core::result::Result::Err(error);
                }
            }),
        );
    }
}
