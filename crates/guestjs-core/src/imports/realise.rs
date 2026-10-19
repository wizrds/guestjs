use crate::{
    backend::{
        modules::BackendModules,
        Backend,
    },
    catalog::ModuleSource,
    enter::Enter,
    errors::Error,
};

pub struct Realiser<'e, 'js, B: Backend> {
    enter: &'e Enter<'js, B>,
}

impl<'e, 'js, B> Realiser<'e, 'js, B>
where
    B: BackendModules,
{
    pub fn new(enter: &'e Enter<'js, B>) -> Self {
        Self {
            enter,
        }
    }

    pub fn realise(
        &self,
        name: &str,
        source: &ModuleSource<B>,
    ) -> Result<B::Module<'js>, Error> {
        match source {
            ModuleSource::Bundle(bundle) => B::declare(
                self.enter.token(),
                name,
                bundle
                    .module(name)
                    .ok_or_else(|| Error::import(name, "the bundle has no module of that name"))?,
            ),
            ModuleSource::Host(module) => {
                B::synthetic(self.enter.token(), name, module.exports(self.enter)?)
            },
            ModuleSource::Native(module) => module.declare(self.enter, name),
        }
    }
}
