use proc_macro2::TokenStream;

mod attributes;
mod callable;
mod class;
mod module;
mod naming;

pub(crate) use crate::host::{
    class::HostClassMacro,
    module::HostModuleMacro,
};

#[derive(Debug)]
pub(crate) enum HostMacroError {
    Attribute(darling::Error),
    Syntax(syn::Error),
}

impl HostMacroError {
    pub(crate) fn write_errors(self) -> TokenStream {
        match self {
            Self::Attribute(error) => error.write_errors(),
            Self::Syntax(error) => error.into_compile_error(),
        }
    }
}

impl From<darling::Error> for HostMacroError {
    fn from(error: darling::Error) -> Self {
        Self::Attribute(error)
    }
}

impl From<syn::Error> for HostMacroError {
    fn from(error: syn::Error) -> Self {
        Self::Syntax(error)
    }
}
