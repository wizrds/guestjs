use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use crate::{
    backend::modules::BackendModules,
    bundle::{Bundle, name::ModuleName},
    errors::Error,
    host::{
        library::{HostInitializer, HostLibrary},
        module::ModuleDeclaration,
        native::{NativeInitializer, NativeLibrary, NativeModule},
    },
    runtime::Runtime,
};

#[derive(Clone, Debug)]
pub struct DeniedNames {
    names: BTreeSet<String>,
}

impl DeniedNames {
    pub fn new() -> Self {
        Self { names: BTreeSet::new() }
    }

    pub fn insert<N>(&mut self, name: N)
    where
        N: Into<String>,
    {
        self.names.insert(name.into());
    }

    pub fn denies(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|denied| ModuleName::new(name).is_within(denied))
    }
}

impl Default for DeniedNames {
    fn default() -> Self {
        Self::new()
    }
}

pub enum ModuleSource<B: BackendModules> {
    Host(Rc<ModuleDeclaration<B>>),
    Native(NativeModule<B>),
    Bundle(Bundle),
}

impl<B: BackendModules> ModuleSource<B> {
    pub fn is_host(&self) -> bool {
        !matches!(self, Self::Bundle(_))
    }
}

impl<B: BackendModules> Clone for ModuleSource<B> {
    fn clone(&self) -> Self {
        match self {
            Self::Host(module) => Self::Host(module.clone()),
            Self::Native(module) => Self::Native(module.clone()),
            Self::Bundle(bundle) => Self::Bundle(bundle.clone()),
        }
    }
}

pub struct Sources<B: BackendModules> {
    modules: BTreeMap<String, ModuleSource<B>>,
    aliases: BTreeMap<String, String>,
    bundles: BTreeMap<String, Bundle>,
}

impl<B: BackendModules> Sources<B> {
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
            aliases: BTreeMap::new(),
            bundles: BTreeMap::new(),
        }
    }

    fn claim(&self, name: &str) -> Result<(), Error> {
        match self.modules.contains_key(name)
            || self.aliases.contains_key(name)
            || self
                .bundles
                .contains_key(ModuleName::new(name).root())
        {
            true => Err(Error::import(name, "the name is already in use")),
            false => Ok(()),
        }
    }

    fn bundle(&self, name: &str) -> Option<&Bundle> {
        self.bundles
            .get(ModuleName::new(name).root())
            .filter(|bundle| bundle.contains(name))
    }

    pub fn insert_host(&mut self, module: Rc<ModuleDeclaration<B>>) -> Result<(), Error> {
        self.claim(module.name())?;

        self.modules
            .insert(module.name().to_owned(), ModuleSource::Host(module));

        Ok(())
    }

    pub fn insert_native(&mut self, module: NativeModule<B>) -> Result<(), Error> {
        self.claim(module.name())?;

        module
            .aliases()
            .iter()
            .try_for_each(|alias| self.claim(alias))?;

        self.aliases.extend(
            module
                .aliases()
                .iter()
                .map(|alias| (alias.clone(), module.name().to_owned())),
        );
        self.modules
            .insert(module.name().to_owned(), ModuleSource::Native(module));

        Ok(())
    }

    pub fn check_bundle(&self, bundle: &Bundle) -> Result<(), Error> {
        bundle
            .roots()
            .into_iter()
            .try_for_each(|root| match self.bundles.get(root) {
                Some(owner) if owner.id() == bundle.id() => Ok(()),
                Some(_) => Err(Error::import(root, "a different bundle already owns this root")),
                None if self
                    .modules
                    .keys()
                    .chain(self.aliases.keys())
                    .any(|name| ModuleName::new(name).is_within(root)) =>
                {
                    Err(Error::import(root, "the root is already in use"))
                }
                None => Ok(()),
            })
    }

    pub fn insert_bundle(&mut self, bundle: &Bundle) -> Result<(), Error> {
        self.check_bundle(bundle)?;

        self.bundles.extend(
            bundle
                .roots()
                .into_iter()
                .map(|root| (root.to_owned(), bundle.clone())),
        );

        Ok(())
    }

    pub fn alias(&self, name: &str) -> Option<&str> {
        self.aliases
            .get(name)
            .map(String::as_str)
    }

    pub fn canonical(&self, name: &str) -> Option<String> {
        match self.modules.contains_key(name) || self.bundle(name).is_some() {
            true => Some(name.to_owned()),
            false => self.alias(name).map(str::to_owned),
        }
    }

    pub fn find(&self, name: &str) -> Option<ModuleSource<B>> {
        self.modules
            .get(name)
            .cloned()
            .or_else(|| {
                self.bundle(name)
                    .map(|bundle| ModuleSource::Bundle(bundle.clone()))
            })
    }
}

impl<B: BackendModules> Default for Sources<B> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Catalog<B: BackendModules> {
    sources: Sources<B>,
    denied: DeniedNames,
    host_initializers: Vec<HostInitializer<B>>,
    native_initializers: Vec<NativeInitializer<B>>,
}

impl<B: BackendModules> Catalog<B> {
    pub fn new() -> Self {
        Self {
            sources: Sources::new(),
            denied: DeniedNames::new(),
            host_initializers: Vec::new(),
            native_initializers: Vec::new(),
        }
    }

    pub fn of(runtime: &Runtime<B>) -> Rc<Self> {
        runtime
            .extensions()
            .get::<Self>()
            .unwrap_or_default()
    }

    pub fn host<L>(mut self, library: L) -> Result<Self, Error>
    where
        L: Into<HostLibrary<B>>,
    {
        let library = library.into();

        library
            .modules()
            .iter()
            .try_for_each(|module| self.sources.insert_host(module.clone()))?;

        self.host_initializers.extend(
            library
                .modules()
                .iter()
                .filter_map(|module| module.initializer())
                .chain(library.initializers())
                .cloned(),
        );

        Ok(self)
    }

    pub fn native<L>(mut self, library: L) -> Result<Self, Error>
    where
        L: Into<NativeLibrary<B>>,
    {
        let library = library.into();

        library
            .modules()
            .iter()
            .try_for_each(|module| {
                self.sources
                    .insert_native(module.clone())
            })?;

        self.native_initializers
            .extend_from_slice(library.initializers());

        Ok(self)
    }

    pub fn bundle(mut self, bundle: &Bundle) -> Result<Self, Error> {
        self.sources.insert_bundle(bundle)?;

        Ok(self)
    }

    pub fn deny<N>(mut self, name: N) -> Self
    where
        N: Into<String>,
    {
        self.denied.insert(name);

        self
    }

    pub fn sources(&self) -> &Sources<B> {
        &self.sources
    }

    pub fn denied(&self) -> &DeniedNames {
        &self.denied
    }

    pub fn host_initializers(&self) -> &[HostInitializer<B>] {
        &self.host_initializers
    }

    pub fn native_initializers(&self) -> &[NativeInitializer<B>] {
        &self.native_initializers
    }
}

impl<B: BackendModules> Default for Catalog<B> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use crate::{
        backend::tests::Stub,
        catalog::{Catalog, DeniedNames},
        enter::Enter,
        guest::Guest,
        host::{
            library::{HostInitializer, HostLibrary},
            module::ModuleDeclaration,
        },
        runtime::Runtime,
    };

    #[test]
    fn nothing_is_denied_by_default() {
        assert!(!DeniedNames::new().denies("fs"));
    }

    #[test]
    fn a_denied_name_denies_everything_within_it() {
        let mut denied = DeniedNames::new();

        denied.insert("fs");

        assert!(denied.denies("fs"));
        assert!(denied.denies("fs/promises"));
        assert!(!denied.denies("fsx"));
        assert!(!denied.denies("path"));
    }

    #[test]
    fn host_collects_module_initializers_before_library_initializers() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let record = |label: &'static str| {
            let order = order.clone();

            move |_: &Enter<'_, Stub>| {
                order.borrow_mut().push(label);

                Ok(())
            }
        };

        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                Catalog::<Stub>::new()
                    .host(
                        HostLibrary::from(HostInitializer::new(record("library")))
                            .extend(ModuleDeclaration::new("first").initialize(record("first")))
                            .extend(ModuleDeclaration::new("plain"))
                            .extend(ModuleDeclaration::new("second").initialize(record("second"))),
                    )?
                    .host_initializers()
                    .iter()
                    .try_for_each(|initializer| initializer.run(enter))
            })
            .unwrap();

        assert_eq!(*order.borrow(), ["first", "second", "library"]);
    }
}
