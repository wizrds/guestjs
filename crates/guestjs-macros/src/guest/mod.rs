use proc_macro2::TokenStream;

mod module;

pub(crate) use crate::guest::module::GuestModuleMacro;

#[derive(Debug)]
pub(crate) enum GuestMacroError {
    Attribute(darling::Error),
    Syntax(syn::Error),
}

impl GuestMacroError {
    pub(crate) fn write_errors(self) -> TokenStream {
        match self {
            Self::Attribute(error) => error.write_errors(),
            Self::Syntax(error) => error.into_compile_error(),
        }
    }
}

impl From<darling::Error> for GuestMacroError {
    fn from(error: darling::Error) -> Self {
        Self::Attribute(error)
    }
}

impl From<syn::Error> for GuestMacroError {
    fn from(error: syn::Error) -> Self {
        Self::Syntax(error)
    }
}
