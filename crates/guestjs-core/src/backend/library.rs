use crate::{
    backend::{Tok, modules::BackendModules},
    errors::Error,
};

pub trait BackendLibrary: BackendModules {
    type NativeModule: Clone + 'static;

    fn declare_native<'js>(
        token: Tok<'js, Self>,
        native: &Self::NativeModule,
        name: &str,
    ) -> Result<Self::Module<'js>, Error>;
}
