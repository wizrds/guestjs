use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    ext::IdentExt,
    parse_quote,
    DeriveInput,
    Generics,
    Ident,
    Path,
};

use crate::{
    attributes::HelperAttributes,
    diagnostics::MacroError,
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct ExceptionOptions {
    name: Option<String>,
    builtin: Option<String>,
    crate_path: Option<Path>,
}

impl ExceptionOptions {
    fn into_parts(self) -> (Option<String>, Option<String>, Option<Path>) {
        (self.name, self.builtin, self.crate_path)
    }
}

pub struct HostExceptionDerive {
    name: String,
    builtin: Ident,
    krate: Path,
    target: Ident,
    generics: Generics,
}

impl HostExceptionDerive {
    pub fn new(input: &DeriveInput) -> Result<Self, MacroError> {
        let (name, builtin, path) = ExceptionOptions::from_list(
            &HelperAttributes::take(&mut input.attrs.clone())?,
        )?.into_parts();
        let builtin = builtin.unwrap_or_else(|| "Error".to_owned());

        if ![
            "Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError",
        ].contains(&builtin.as_str()) {
            return Err(syn::Error::new(
                input.ident.span(),
                "unknown JavaScript built-in error kind",
            ).into());
        }

        Ok(Self {
            name: name.unwrap_or_else(|| input.ident.unraw().to_string()),
            builtin: Ident::new(&builtin, input.ident.span()),
            krate: CratePath::new(path).resolve()?,
            target: input.ident.clone(),
            generics: input.generics.clone(),
        })
    }

    pub fn expand(self) -> TokenStream {
        let Self {
            name,
            builtin,
            krate,
            target,
            mut generics,
        } = self;
        let source = generics.clone();
        let (_, target_generics, _) = source.split_for_impl();

        generics.make_where_clause().predicates.push(
            parse_quote!(#target #target_generics: 'static),
        );

        let (implementation, _, clause) = generics.split_for_impl();

        quote! {
            impl #implementation #krate::host::exception::HostException
                for #target #target_generics #clause
            {
                const NAME: &'static str = #name;
                const BASE: #krate::errors::GuestErrorKind =
                    #krate::errors::GuestErrorKind::#builtin;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::{
        parse2,
        parse_quote,
        ItemImpl,
    };

    use crate::host::exception::HostExceptionDerive;

    #[test]
    fn emits_only_the_host_exception_contract() {
        assert_eq!(
            parse2::<ItemImpl>(
                HostExceptionDerive::new(&parse_quote!(
                    #[guestjs(
                        crate_path = crate,
                        name = "ValidationError",
                        builtin = "TypeError",
                    )]
                    struct Invalid;
                ))
                .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
                .expand(),
            ).unwrap(),
            parse_quote!(
                impl crate::host::exception::HostException for Invalid
                where
                    Invalid: 'static
                {
                    const NAME: &'static str = "ValidationError";
                    const BASE: crate::errors::GuestErrorKind =
                        crate::errors::GuestErrorKind::TypeError;
                }
            ),
        );
    }

    #[test]
    fn rejects_unknown_builtins() {
        assert!(HostExceptionDerive::new(&parse_quote!(
            #[guestjs(crate_path = crate, builtin = "ValueError")]
            struct Invalid;
        )).is_err());
    }
}
