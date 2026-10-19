use std::{
    cell::RefCell,
    rc::Rc,
};

use crate::{
    backend::modules::BackendModules,
    bundle::Bundle,
    catalog::{
        Catalog,
        ModuleSource,
        Sources,
    },
    errors::Error,
    guest::Guest,
};

pub struct GuestBindings<B: BackendModules> {
    runtime: Rc<Catalog<B>>,
    guest: Catalog<B>,
    mounted: RefCell<Sources<B>>,
}

impl<B: BackendModules> GuestBindings<B> {
    pub fn new(runtime: Rc<Catalog<B>>, guest: Catalog<B>) -> Self {
        Self {
            runtime,
            guest,
            mounted: RefCell::new(Sources::new()),
        }
    }

    pub fn of(guest: &Guest<B>) -> Rc<Self> {
        guest
            .extensions()
            .get_or_insert_with(|| Self::new(Catalog::of(guest.runtime()), Catalog::new()))
    }

    fn first<T, F>(&self, find: F) -> Option<T>
    where
        F: Fn(&Sources<B>) -> Option<T>,
    {
        find(&self.mounted.borrow())
            .or_else(|| find(self.guest.sources()))
            .or_else(|| find(self.runtime.sources()))
    }

    pub fn runtime_catalog(&self) -> &Catalog<B> {
        &self.runtime
    }

    pub fn guest_catalog(&self) -> &Catalog<B> {
        &self.guest
    }

    pub fn source(&self, name: &str) -> Option<ModuleSource<B>> {
        self.first(|sources| sources.find(name))
    }

    pub fn is_denied(&self, name: &str) -> bool {
        self.guest.denied().denies(name) || self.runtime.denied().denies(name)
    }

    pub fn locate(&self, name: &str) -> Result<String, Error> {
        [name.to_owned(), format!("{name}/index")]
            .into_iter()
            .find_map(|candidate| {
                self.first(|sources| sources.canonical(&candidate))
            })
            .ok_or_else(|| {
                Error::import(
                    name,
                    "no module of that name is bound to this guest",
                )
            })
            .and_then(|located| {
                match self.is_denied(name) || self.is_denied(&located) {
                    true => Err(Error::import(
                        name,
                        "the name is denied to this guest",
                    )),
                    false => Ok(located),
                }
            })
    }

    pub fn is_host_module(&self, name: &str) -> bool {
        self.locate(name)
            .ok()
            .and_then(|located| self.source(&located))
            .is_some_and(|source| source.is_host())
    }

    pub fn mount(&self, bundle: &Bundle) -> Result<(), Error> {
        self.runtime.sources().check_bundle(bundle)?;
        self.guest.sources().check_bundle(bundle)?;

        self.mounted
            .borrow_mut()
            .insert_bundle(bundle)
    }
}
