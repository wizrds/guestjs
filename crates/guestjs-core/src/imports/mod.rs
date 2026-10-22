pub mod bindings;
pub mod realise;
pub mod resolver;

use std::rc::Rc;

use crate::{
    backend::{
        Backend, Tok,
        exceptions::BackendExceptions,
        modules::{BackendModules, ModuleDispatcher},
        promises::{BackendPromises, PromiseState},
    },
    bundle::Bundle,
    catalog::Catalog,
    errors::Error,
    guest::{Guest, registry::GuestRegistry},
    handle::module::Module,
    imports::{bindings::GuestBindings, realise::Realiser, resolver::Resolver},
    marshal::FromGuest,
    runtime::Runtime,
};

pub struct Imports<B: Backend> {
    registry: Rc<GuestRegistry<B>>,
}

impl<B: BackendModules> Imports<B> {
    pub fn new(registry: Rc<GuestRegistry<B>>) -> Self {
        Self { registry }
    }

    fn guest<'js>(&self, token: Tok<'js, B>) -> Result<Guest<B>, Error> {
        self.registry
            .find(B::context_id(token))
            .ok_or_else(|| {
                Error::unexpected("a module was requested by a context that has no guest")
            })
    }

    pub fn install(runtime: &Runtime<B>, catalog: Catalog<B>) -> Result<(), Error> {
        B::install(runtime.engine(), Rc::new(Self::new(runtime.registry().clone())))?;

        runtime.extensions().insert(catalog);

        Ok(())
    }
}

impl<B: BackendModules> ModuleDispatcher<B> for Imports<B> {
    fn resolve<'js>(
        &self,
        token: Tok<'js, B>,
        referrer: &str,
        specifier: &str,
    ) -> Result<String, Error> {
        self.guest(token)?
            .reenter(token, |enter| {
                GuestBindings::of(enter.guest())
                    .locate(&Resolver::new().resolve(referrer, specifier)?)
            })
    }

    fn load<'js>(&self, token: Tok<'js, B>, name: &str) -> Result<B::Module<'js>, Error> {
        self.guest(token)?
            .reenter(token, |enter| {
                Realiser::new(enter).realise(
                    name,
                    &GuestBindings::of(enter.guest())
                        .source(name)
                        .ok_or_else(|| {
                            Error::import(name, "no module of that name is bound to this guest")
                        })?,
                )
            })
    }
}

pub struct GuestModules<'g, B: Backend> {
    guest: &'g Guest<B>,
}

impl<'g, B> GuestModules<'g, B>
where
    B: BackendModules + BackendPromises + BackendExceptions,
{
    pub fn new(guest: &'g Guest<B>) -> Self {
        Self { guest }
    }

    pub fn import(&self, name: &str) -> Result<Module<B>, Error> {
        self.guest.enter(|enter| {
            let (namespace, evaluation) =
                B::import(enter.token(), &Resolver::new().resolve("", name)?)?;

            match B::state(enter.token(), &evaluation)? {
                PromiseState::Fulfilled(_) => Module::from_guest(enter, namespace),
                PromiseState::Pending => Module::evaluating(enter, namespace, evaluation),
                PromiseState::Rejected(reason) => Err(B::throw(enter.token(), reason)),
            }
        })
    }

    pub fn load(&self, bundle: &Bundle) -> Result<Module<B>, Error> {
        match bundle.root() {
            Some(root) => {
                GuestBindings::of(self.guest).mount(bundle)?;

                self.import(root)
            }
            None => Err(Error::bundle(format!(
                "a bundle with the roots {:?} has no single root to load",
                bundle.roots(),
            ))),
        }
    }

    pub fn guest_module<P, S>(&self, path: P, source: S) -> Result<Module<B>, Error>
    where
        P: AsRef<str>,
        S: AsRef<str>,
    {
        self.load(&Bundle::single(path, source)?)
    }

    pub fn host_module(&self, name: &str) -> Result<Module<B>, Error> {
        match GuestBindings::of(self.guest).is_host_module(name) {
            true => self.import(name),
            false => Err(Error::import(name, "no host module of that name is bound to this guest")),
        }
    }
}

pub trait GuestModulesExt<B>
where
    B: BackendModules + BackendPromises + BackendExceptions,
{
    fn import(&self, name: &str) -> Result<Module<B>, Error>;

    fn load(&self, bundle: &Bundle) -> Result<Module<B>, Error>;

    fn guest_module<P, S>(&self, path: P, source: S) -> Result<Module<B>, Error>
    where
        P: AsRef<str>,
        S: AsRef<str>;

    fn host_module(&self, name: &str) -> Result<Module<B>, Error>;
}

impl<B> GuestModulesExt<B> for Guest<B>
where
    B: BackendModules + BackendPromises + BackendExceptions,
{
    fn import(&self, name: &str) -> Result<Module<B>, Error> {
        GuestModules::new(self).import(name)
    }

    fn load(&self, bundle: &Bundle) -> Result<Module<B>, Error> {
        GuestModules::new(self).load(bundle)
    }

    fn guest_module<P, S>(&self, path: P, source: S) -> Result<Module<B>, Error>
    where
        P: AsRef<str>,
        S: AsRef<str>,
    {
        GuestModules::new(self).guest_module(path, source)
    }

    fn host_module(&self, name: &str) -> Result<Module<B>, Error> {
        GuestModules::new(self).host_module(name)
    }
}
