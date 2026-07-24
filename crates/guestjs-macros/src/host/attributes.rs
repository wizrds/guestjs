use darling::ast::NestedMeta;
use syn::{Attribute, Meta};

use crate::host::HostMacroError;

pub(super) struct HelperAttributes;

impl HelperAttributes {
    pub(super) fn take(
        attrs: &mut Vec<Attribute>,
    ) -> Result<Vec<NestedMeta>, HostMacroError> {
        let mut helpers = Vec::new();
        let mut retained = Vec::with_capacity(attrs.len());

        for attr in attrs.drain(..) {
            if !attr.path().is_ident("guestjs") {
                retained.push(attr);

                continue;
            }

            match attr.meta {
                Meta::List(list) => {
                    helpers.extend(NestedMeta::parse_meta_list(list.tokens)?);
                }
                meta => {
                    return Err(syn::Error::new_spanned(
                        meta,
                        "expected #[guestjs(...)]",
                    )
                    .into());
                }
            }
        }

        *attrs = retained;

        Ok(helpers)
    }
}

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::{ImplItemFn, parse_quote};

    use crate::host::attributes::HelperAttributes;

    #[test]
    fn consumes_guestjs_and_preserves_other_attributes() {
        let mut method: ImplItemFn = parse_quote! {
            #[allow(dead_code)]
            #[guestjs(method, name = "read")]
            fn read(&self) -> Result<i32, Error> {
                Ok(1)
            }
        };

        assert_eq!(
            HelperAttributes::take(&mut method.attrs)
                .unwrap()
                .len(),
            2,
        );
        assert_eq!(method.attrs.len(), 1);
        assert!(
            method.attrs[0]
                .to_token_stream()
                .to_string()
                .contains("allow"),
        );
    }
}
