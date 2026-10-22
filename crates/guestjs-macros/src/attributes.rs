use darling::ast::NestedMeta;
use syn::{Attribute, Meta};

pub struct HelperAttributes;

impl HelperAttributes {
    pub fn conditions(attributes: &[Attribute]) -> Vec<Attribute> {
        attributes
            .iter()
            .filter(|attribute| {
                attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
            })
            .cloned()
            .collect()
    }

    pub fn take(attributes: &mut Vec<Attribute>) -> Result<Vec<NestedMeta>, darling::Error> {
        let mut helpers = Vec::new();
        let mut retained = Vec::with_capacity(attributes.len());

        for attribute in attributes.drain(..) {
            if !attribute.path().is_ident("guestjs") {
                retained.push(attribute);

                continue;
            }

            match attribute.meta {
                Meta::List(list) => {
                    helpers.extend(NestedMeta::parse_meta_list(list.tokens)?);
                }
                meta => {
                    return Err(syn::Error::new_spanned(meta, "expected #[guestjs(...)]").into());
                }
            }
        }

        *attributes = retained;

        Ok(helpers)
    }
}

#[cfg(test)]
mod tests {
    use syn::{Attribute, parse_quote};

    use crate::attributes::HelperAttributes;

    #[test]
    fn consumes_helpers_and_preserves_other_attributes_in_order() {
        let retained: Vec<Attribute> = vec![
            parse_quote!(#[cfg(feature = "feature")]),
            parse_quote!(#[allow(dead_code)]),
        ];
        let mut attributes = vec![
            retained[0].clone(),
            parse_quote!(#[guestjs(method)]),
            retained[1].clone(),
            parse_quote!(#[guestjs(name = "send")]),
        ];

        assert_eq!(
            HelperAttributes::take(&mut attributes)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(attributes, retained);
    }

    #[test]
    fn rejects_non_list_helpers() {
        for attribute in [
            parse_quote!(#[guestjs]),
            parse_quote!(#[guestjs = "method"]),
        ] {
            assert!(HelperAttributes::take(&mut vec![attribute],).is_err());
        }
    }
}
