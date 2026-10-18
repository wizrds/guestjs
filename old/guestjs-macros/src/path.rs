use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use syn::{Ident, Path, parse_quote};

pub(crate) struct CratePath {
    explicit: Option<Path>,
}

impl CratePath {
    pub(crate) fn new(explicit: Option<Path>) -> Self {
        Self { explicit }
    }

    fn from_found(found: FoundCrate) -> Path {
        match found {
            FoundCrate::Itself => parse_quote!(crate),
            FoundCrate::Name(name) => {
                let mut path: Path = parse_quote!(::guestjs);

                path.segments[0].ident = Ident::new(&name.replace('-', "_"), Span::call_site());

                path
            }
        }
    }

    pub(crate) fn resolve(self) -> Result<Path, syn::Error> {
        match self.explicit {
            Some(path) => Ok(path),
            None => crate_name("guestjs")
                .map(Self::from_found)
                .map_err(|error| syn::Error::new(Span::call_site(), error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro_crate::FoundCrate;
    use quote::ToTokens;
    use syn::parse_quote;

    use crate::path::CratePath;

    #[test]
    fn resolves_current_crate() {
        assert_eq!(
            CratePath::from_found(FoundCrate::Itself)
                .into_token_stream()
                .to_string(),
            "crate",
        );
    }

    #[test]
    fn resolves_renamed_dependency() {
        assert_eq!(
            CratePath::from_found(FoundCrate::Name(String::from("js")))
                .into_token_stream()
                .to_string(),
            ":: js",
        );
    }

    #[test]
    fn preserves_explicit_path() {
        assert_eq!(
            CratePath::new(Some(parse_quote!(custom::guestjs)))
                .resolve()
                .unwrap()
                .into_token_stream()
                .to_string(),
            "custom :: guestjs",
        );
    }
}
