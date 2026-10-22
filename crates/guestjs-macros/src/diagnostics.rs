use darling::Error as AttributeError;
use proc_macro2::TokenStream;
use syn::Error as SyntaxError;

pub enum MacroError {
    Attribute(AttributeError),
    Syntax(SyntaxError),
}

impl MacroError {
    pub fn into_tokens(self) -> TokenStream {
        match self {
            Self::Attribute(error) => error.write_errors(),
            Self::Syntax(error) => error.into_compile_error(),
        }
    }
}

impl From<AttributeError> for MacroError {
    fn from(error: AttributeError) -> Self {
        Self::Attribute(error)
    }
}

impl From<SyntaxError> for MacroError {
    fn from(error: SyntaxError) -> Self {
        Self::Syntax(error)
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::Span;
    use syn::{ItemMacro, LitStr, parse2};

    use crate::diagnostics::MacroError;

    #[test]
    fn syntax_errors_become_compile_errors() {
        let emitted = parse2::<ItemMacro>(
            MacroError::from(syn::Error::new(Span::call_site(), "invalid syntax")).into_tokens(),
        )
        .unwrap();

        assert_eq!(
            emitted
                .mac
                .path
                .segments
                .last()
                .unwrap()
                .ident,
            "compile_error",
        );
        assert_eq!(
            parse2::<LitStr>(emitted.mac.tokens)
                .unwrap()
                .value(),
            "invalid syntax",
        );
    }

    #[test]
    fn attribute_errors_become_compile_errors() {
        let emitted = parse2::<ItemMacro>(
            MacroError::from(darling::Error::custom("invalid attribute")).into_tokens(),
        )
        .unwrap();

        assert_eq!(
            emitted
                .mac
                .path
                .segments
                .last()
                .unwrap()
                .ident,
            "compile_error",
        );
        assert_eq!(
            parse2::<LitStr>(emitted.mac.tokens)
                .unwrap()
                .value(),
            "invalid attribute",
        );
    }
}
