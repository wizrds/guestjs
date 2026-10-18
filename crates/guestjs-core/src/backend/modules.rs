use std::rc::Rc;

use crate::{
    backend::{
        values::BackendValues,
        Tok,
        Val,
    },
    errors::Error,
};

pub trait ModuleDispatcher<B: BackendModules> {
    fn resolve<'js>(
        &self,
        token: Tok<'js, B>,
        referrer: &str,
        specifier: &str,
    ) -> Result<String, Error>;

    fn load<'js>(
        &self,
        token: Tok<'js, B>,
        name: &str,
    ) -> Result<B::Module<'js>, Error>;
}

pub trait BackendModules: BackendValues {
    type Module<'js>;

    fn eval<'js>(
        token: Tok<'js, Self>,
        source: &str,
        name: &str,
    ) -> Result<Val<'js, Self>, Error>;

    fn declare<'js>(
        token: Tok<'js, Self>,
        name: &str,
        source: &str,
    ) -> Result<Self::Module<'js>, Error>;

    fn synthetic<'js>(
        token: Tok<'js, Self>,
        name: &str,
        exports: Vec<(String, Val<'js, Self>)>,
    ) -> Result<Self::Module<'js>, Error>;

    fn evaluate<'js>(
        token: Tok<'js, Self>,
        module: Self::Module<'js>,
    ) -> Result<(Val<'js, Self>, Val<'js, Self>), Error>;

    fn install(
        engine: &Self::Engine,
        dispatcher: Rc<dyn ModuleDispatcher<Self>>,
    ) -> Result<(), Error>;
}
