use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::{
        Parse,
        ParseStream,
    },
    parse2,
    Error,
    Ident,
    LitStr,
    Path,
    Result,
    Token,
};

use crate::path::CratePath;

#[derive(Debug)]
pub struct BundleMacro {
    path: LitStr,
    crate_path: Path,
}

impl BundleMacro {
    pub fn new(tokens: TokenStream) -> Result<Self> {
        parse2(tokens)
    }

    pub fn expand(self) -> TokenStream {
        let Self { path, crate_path } = self;

        quote! {{
            use #crate_path::embed::__include_dir as include_dir;

            const __GUESTJS_EMBEDDED: #crate_path::embed::Dir<'static> =
                #crate_path::embed::__include_dir_macro!(#path);

            #crate_path::bundle::Bundle::from_embedded(
                &__GUESTJS_EMBEDDED,
            )
        }}
    }
}

impl Parse for BundleMacro {
    fn parse(input: ParseStream) -> Result<Self> {
        let path = input.parse::<LitStr>()?;
        let mut crate_path = None;

        if !input.is_empty() {
            input.parse::<Token![,]>()?;

            if !input.is_empty() {
                let option = input.parse::<Ident>()?;

                if option != "crate_path" {
                    return Err(Error::new(
                        option.span(),
                        "expected `crate_path`",
                    ));
                }

                input.parse::<Token![=]>()?;
                crate_path = Some(input.parse::<Path>()?);

                if !input.is_empty() {
                    input.parse::<Token![,]>()?;
                }

                if !input.is_empty() {
                    return Err(input.error("unexpected bundle option"));
                }
            }
        }

        Ok(Self {
            path: LitStr::new(&path.value(), path.span()),
            crate_path: CratePath::new(crate_path).resolve()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{
        parse2,
        Expr,
    };

    use crate::bundle::BundleMacro;

    #[test]
    fn expansion_delegates_embedding_and_bundle_construction() {
        assert_eq!(
            parse2::<Expr>(
                BundleMacro::new(quote!(
                    "$CARGO_MANIFEST_DIR/guest",
                    crate_path = custom::guestjs,
                ))
                .unwrap()
                .expand(),
            )
            .unwrap(),
            parse2::<Expr>(quote! {{
                use custom::guestjs::embed::__include_dir as include_dir;

                const __GUESTJS_EMBEDDED:
                    custom::guestjs::embed::Dir<'static> =
                    custom::guestjs::embed::__include_dir_macro!(
                        "$CARGO_MANIFEST_DIR/guest"
                    );

                custom::guestjs::bundle::Bundle::from_embedded(
                    &__GUESTJS_EMBEDDED,
                )
            }})
            .unwrap(),
        );
    }

    #[test]
    fn raw_literals_are_emitted_as_ordinary_literals() {
        assert_eq!(
            BundleMacro::new(quote!(
                r"$CARGO_MANIFEST_DIR/guest",
                crate_path = custom::guestjs,
            ))
            .unwrap()
            .expand()
            .to_string(),
            BundleMacro::new(quote!(
                "$CARGO_MANIFEST_DIR/guest",
                crate_path = custom::guestjs,
            ))
            .unwrap()
            .expand()
            .to_string(),
        );
    }

    #[test]
    fn invalid_options_and_extra_tokens_are_rejected() {
        [
            quote!("guest", root = "app"),
            quote!(
                "guest",
                crate_path = first::guestjs,
                crate_path = second::guestjs,
            ),
            quote!("guest", crate_path = custom::guestjs, extra),
            quote!(123),
        ]
        .into_iter()
        .for_each(|tokens| {
            assert!(BundleMacro::new(tokens).is_err());
        });
    }
}
