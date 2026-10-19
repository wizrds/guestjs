use proc_macro_crate::{
    crate_name,
    FoundCrate,
};
use proc_macro2::Span;
use syn::{
    parse_quote,
    parse_str,
    Error,
    Path,
    Result,
};

pub struct CratePath {
    explicit: Option<Path>,
}

impl CratePath {
    pub fn new(explicit: Option<Path>) -> Self {
        Self { explicit }
    }

    pub fn resolve(self) -> Result<Path> {
        if let Some(path) = self.explicit {
            return Ok(path);
        }

        match crate_name("guestjs") {
            Ok(FoundCrate::Itself) => Ok(parse_quote!(::guestjs)),
            Ok(FoundCrate::Name(name)) => {
                parse_str(&format!("::{}", name.replace('-', "_")))
            },
            Err(error) => Err(Error::new(
                Span::call_site(),
                format!(
                    "cannot resolve guestjs: {error}; supply crate_path explicitly",
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use crate::path::CratePath;

    #[test]
    fn explicit_facade_path_is_preserved() {
        assert_eq!(
            CratePath::new(Some(parse_quote!(custom::guestjs)))
                .resolve()
                .unwrap(),
            parse_quote!(custom::guestjs),
        );
    }
}
