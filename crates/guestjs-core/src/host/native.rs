use std::rc::Rc;

use crate::{
    backend::{Backend, library::BackendLibrary, modules::BackendModules},
    enter::Enter,
    errors::Error,
};

pub struct NativeModule<B: BackendModules> {
    name: String,
    aliases: Vec<String>,
    declare: Rc<dyn for<'js> Fn(&Enter<'js, B>, &str) -> Result<B::Module<'js>, Error>>,
}

impl<B: BackendModules> NativeModule<B> {
    pub fn new<N>(name: N, native: B::NativeModule) -> Self
    where
        B: BackendLibrary,
        N: Into<String>,
    {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            declare: Rc::new(move |enter, name| B::declare_native(enter.token(), &native, name)),
        }
    }

    pub fn alias<N>(mut self, alias: N) -> Self
    where
        N: Into<String>,
    {
        let alias = alias.into();

        if alias != self.name && !self.aliases.contains(&alias) {
            self.aliases.push(alias);
        }

        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub fn declare<'js>(&self, enter: &Enter<'js, B>, name: &str) -> Result<B::Module<'js>, Error> {
        (self.declare)(enter, name)
    }
}

impl<B: BackendModules> Clone for NativeModule<B> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            aliases: self.aliases.clone(),
            declare: self.declare.clone(),
        }
    }
}

pub struct NativeInitializer<B: Backend> {
    initialize: Rc<dyn for<'js> Fn(&Enter<'js, B>) -> Result<(), Error>>,
}

impl<B: Backend> NativeInitializer<B> {
    pub fn new<F>(initialize: F) -> Self
    where
        B: BackendLibrary,
        F: for<'js> Fn(&Enter<'js, B>) -> Result<(), Error> + 'static,
    {
        Self { initialize: Rc::new(initialize) }
    }

    pub fn run<'js>(&self, enter: &Enter<'js, B>) -> Result<(), Error> {
        (self.initialize)(enter)
    }
}

impl<B: Backend> Clone for NativeInitializer<B> {
    fn clone(&self) -> Self {
        Self { initialize: self.initialize.clone() }
    }
}

pub struct NativeLibrary<B: BackendModules> {
    modules: Vec<NativeModule<B>>,
    initializers: Vec<NativeInitializer<B>>,
}

impl<B: BackendModules> NativeLibrary<B> {
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            initializers: Vec::new(),
        }
    }

    pub fn extend<L>(mut self, library: L) -> Self
    where
        L: Into<NativeLibrary<B>>,
    {
        let library = library.into();

        self.modules.extend(library.modules);
        self.initializers
            .extend(library.initializers);

        self
    }

    pub fn modules(&self) -> &[NativeModule<B>] {
        &self.modules
    }

    pub fn initializers(&self) -> &[NativeInitializer<B>] {
        &self.initializers
    }
}

impl<B: BackendModules> Default for NativeLibrary<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: BackendModules> Clone for NativeLibrary<B> {
    fn clone(&self) -> Self {
        Self {
            modules: self.modules.clone(),
            initializers: self.initializers.clone(),
        }
    }
}

impl<B: BackendModules> From<NativeModule<B>> for NativeLibrary<B> {
    fn from(module: NativeModule<B>) -> Self {
        Self {
            modules: vec![module],
            initializers: Vec::new(),
        }
    }
}

impl<B: BackendModules> From<NativeInitializer<B>> for NativeLibrary<B> {
    fn from(initializer: NativeInitializer<B>) -> Self {
        Self {
            modules: Vec::new(),
            initializers: vec![initializer],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::library::BackendLibrary,
        host::native::{NativeInitializer, NativeLibrary, NativeModule},
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendLibrary>(native: B::NativeModule) {
        fn clone<T: Clone>() {}

        NativeLibrary::new()
            .extend(
                NativeModule::<B>::new("fs", native)
                    .alias("node:fs")
                    .alias("fs"),
            )
            .extend(NativeInitializer::new(|_| Ok(())));

        clone::<NativeModule<B>>();
        clone::<NativeInitializer<B>>();
        clone::<NativeLibrary<B>>();
    }
}
