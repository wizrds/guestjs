pub mod name;
pub mod transpiler;

use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fs,
    io,
    num::NonZeroU64,
    path::Path,
    sync::{
        atomic::{
            AtomicU64,
            Ordering,
        },
        Arc,
    },
};

use crate::{
    bundle::{
        name::ModuleName,
        transpiler::Transpiler,
    },
    errors::Error,
};

struct BundleState {
    id: BundleId,
    modules: BTreeMap<String, Arc<str>>,
    data: BTreeMap<String, Arc<[u8]>>,
}

impl BundleState {
    pub fn new(
        modules: BTreeMap<String, Arc<str>>,
        data: BTreeMap<String, Arc<[u8]>>,
    ) -> Self {
        Self {
            id: BundleId::next(),
            modules,
            data,
        }
    }

    pub fn id(&self) -> BundleId {
        self.id
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.modules
            .keys()
            .map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    pub fn module(&self, name: &str) -> Option<&str> {
        self.modules
            .get(name)
            .map(AsRef::as_ref)
    }

    pub fn data(&self, path: &str) -> Option<&[u8]> {
        self.data
            .get(path)
            .map(AsRef::as_ref)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BundleId(NonZeroU64);

impl BundleId {
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);

        Self(
            NonZeroU64::new(NEXT.fetch_add(1, Ordering::Relaxed))
                .expect("bundle id counter overflowed"),
        )
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BundlePath<'a> {
    path: &'a str,
}

impl<'a> BundlePath<'a> {
    pub fn new(path: &'a str) -> Result<Self, Error> {
        match path
            .split('/')
            .all(|segment| {
                !matches!(segment, "" | "." | "..")
                    && !segment.contains('\\')
            })
        {
            true => Ok(Self {
                path,
            }),
            false => Err(Error::bundle(format!(
                "{path:?} is not a relative path of named segments",
            ))),
        }
    }

    pub fn as_str(&self) -> &'a str {
        self.path
    }

    pub fn module_name(&self) -> Option<ModuleName<'a>> {
        ModuleName::from_path(self.path)
    }
}

#[derive(Clone)]
pub struct Bundle {
    state: Arc<BundleState>,
}

impl Bundle {
    fn new(
        modules: BTreeMap<String, Arc<str>>,
        data: BTreeMap<String, Arc<[u8]>>,
    ) -> Self {
        Self {
            state: Arc::new(BundleState::new(modules, data)),
        }
    }

    pub fn single<P, S>(path: P, source: S) -> Result<Self, Error>
    where
        P: AsRef<str>,
        S: AsRef<str>,
    {
        Self::builder()
            .module(path, source)
            .build()
    }

    pub fn from_files<I, P, C>(files: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = (P, C)>,
        P: AsRef<str>,
        C: AsRef<[u8]>,
    {
        files
            .into_iter()
            .fold(
                Self::builder(),
                |builder, (path, contents)| builder.file(path, contents),
            )
            .build()
    }

    pub fn from_dir<P>(directory: P) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        Self::builder()
            .directory(directory)
            .build()
    }

    pub fn builder() -> BundleBuilder {
        BundleBuilder::new()
    }

    pub fn id(&self) -> BundleId {
        self.state.id()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.state.names()
    }

    pub fn roots(&self) -> Vec<&str> {
        self.names()
            .map(|name| ModuleName::new(name).root())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn root(&self) -> Option<&str> {
        match self.roots().as_slice() {
            [root] => Some(*root),
            _ => None,
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.state.contains(name)
    }

    pub fn module(&self, name: &str) -> Option<&str> {
        self.state.module(name)
    }

    pub fn data(&self, path: &str) -> Option<&[u8]> {
        self.state.data(path)
    }
}

pub struct BundleBuilder {
    modules: BTreeMap<String, Arc<str>>,
    data: BTreeMap<String, Arc<[u8]>>,
    error: Option<Error>,
}

impl BundleBuilder {
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
            data: BTreeMap::new(),
            error: None,
        }
    }

    fn unreadable(
        path: &Path,
        error: io::Error,
    ) -> Error {
        Error::bundle(format!("{}: {error}", path.display()))
    }

    fn attempt<F>(mut self, operation: F) -> Self
    where
        F: FnOnce(&mut Self) -> Result<(), Error>,
    {
        if self.error.is_none() {
            self.error = operation(&mut self).err();
        }

        self
    }

    fn insert_module(
        &mut self,
        path: BundlePath,
        source: &str,
    ) -> Result<(), Error> {
        let name = path
            .module_name()
            .ok_or_else(|| {
                Error::bundle(format!(
                    "{:?} is not a JavaScript or TypeScript module",
                    path.as_str(),
                ))
            })?;

        match self.modules.contains_key(name.as_str()) {
            true => Err(Error::bundle(format!(
                "{:?} names the module {:?} a second time",
                path.as_str(),
                name.as_str(),
            ))),
            false => {
                self.modules.insert(
                    name.as_str().to_owned(),
                    Transpiler::new()
                        .transpile(path.as_str(), source)?
                        .into(),
                );

                Ok(())
            },
        }
    }

    fn insert_data(
        &mut self,
        path: BundlePath,
        contents: &[u8],
    ) -> Result<(), Error> {
        match self.data.contains_key(path.as_str()) {
            true => Err(Error::bundle(format!(
                "{:?} is added a second time",
                path.as_str(),
            ))),
            false => {
                self.data.insert(path.as_str().to_owned(), contents.into());

                Ok(())
            },
        }
    }

    fn insert_file(
        &mut self,
        path: BundlePath,
        contents: &[u8],
    ) -> Result<(), Error> {
        match path.module_name() {
            Some(_) => self.insert_module(
                path,
                std::str::from_utf8(contents).map_err(|error| {
                    Error::bundle(format!(
                        "{:?} is not UTF-8: {error}",
                        path.as_str(),
                    ))
                })?,
            ),
            None => self.insert_data(path, contents),
        }
    }

    fn insert_directory(
        &mut self,
        root: &Path,
        directory: &Path,
    ) -> Result<(), Error> {
        fs::read_dir(directory)
            .map_err(|error| Self::unreadable(directory, error))?
            .try_for_each(|entry| {
                let path = entry
                    .map_err(|error| Self::unreadable(directory, error))?
                    .path();

                match path.is_dir() {
                    true => self.insert_directory(root, &path),
                    false => self.insert_file(
                        BundlePath::new(
                            &path
                                .strip_prefix(root)
                                .ok()
                                .and_then(|relative| {
                                    relative
                                        .components()
                                        .map(|component| {
                                            component.as_os_str().to_str()
                                        })
                                        .collect::<Option<Vec<_>>>()
                                })
                                .ok_or_else(|| {
                                    Error::bundle(format!(
                                        "{} is not a UTF-8 path under {}",
                                        path.display(),
                                        root.display(),
                                    ))
                                })?
                                .join("/"),
                        )?,
                        &fs::read(&path)
                            .map_err(|error| Self::unreadable(&path, error))?,
                    ),
                }
            })
    }

    pub fn module<P, S>(self, path: P, source: S) -> Self
    where
        P: AsRef<str>,
        S: AsRef<str>,
    {
        self.attempt(|builder| {
            builder.insert_module(
                BundlePath::new(path.as_ref())?,
                source.as_ref(),
            )
        })
    }

    pub fn data<P, C>(self, path: P, contents: C) -> Self
    where
        P: AsRef<str>,
        C: AsRef<[u8]>,
    {
        self.attempt(|builder| {
            builder.insert_data(
                BundlePath::new(path.as_ref())?,
                contents.as_ref(),
            )
        })
    }

    pub fn file<P, C>(self, path: P, contents: C) -> Self
    where
        P: AsRef<str>,
        C: AsRef<[u8]>,
    {
        self.attempt(|builder| {
            builder.insert_file(
                BundlePath::new(path.as_ref())?,
                contents.as_ref(),
            )
        })
    }

    pub fn directory<P>(self, directory: P) -> Self
    where
        P: AsRef<Path>,
    {
        self.attempt(|builder| {
            builder.insert_directory(directory.as_ref(), directory.as_ref())
        })
    }

    pub fn build(self) -> Result<Bundle, Error> {
        match (self.error, self.modules.is_empty()) {
            (Some(error), _) => Err(error),
            (None, true) => {
                Err(Error::bundle("a bundle needs at least one module"))
            },
            (None, false) => Ok(Bundle::new(self.modules, self.data)),
        }
    }
}

impl Default for BundleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::bundle::{
        Bundle,
        BundlePath,
    };

    #[test]
    fn paths_of_named_segments_are_accepted() {
        assert_eq!(
            BundlePath::new("plugin/util.ts")
                .unwrap()
                .module_name()
                .map(|name| name.as_str()),
            Some("plugin/util"),
        );
    }

    #[test]
    fn paths_with_empty_dot_or_backslash_segments_are_rejected() {
        ["", "/a.js", "a//b.js", "./a.js", "a/../b.js", "a\\b.js"]
            .into_iter()
            .for_each(|path| {
                assert!(
                    BundlePath::new(path).is_err_and(|error| error.is_bundle()),
                    "{path}",
                );
            });
    }

    #[test]
    fn every_bundle_has_its_own_id() {
        let first = Bundle::single("a.js", "").unwrap();

        assert_eq!(first.clone().id(), first.id());
        assert_ne!(Bundle::single("a.js", "").unwrap().id(), first.id());
    }

    #[test]
    fn typescript_is_transpiled_when_added() {
        assert!(!Bundle::single("main.ts", "export const x: number = 1;")
            .unwrap()
            .module("main")
            .unwrap()
            .contains(": number"));
    }

    #[test]
    fn one_top_level_name_is_the_root() {
        let bundle = Bundle::builder()
            .module("plugin/index.js", "")
            .module("plugin/util.js", "")
            .build()
            .unwrap();

        assert_eq!(bundle.roots(), ["plugin"]);
        assert_eq!(bundle.root(), Some("plugin"));
    }

    #[test]
    fn several_top_level_names_have_no_root() {
        let bundle = Bundle::builder()
            .module("a.js", "")
            .module("a-b/index.js", "")
            .module("a/index.js", "")
            .build()
            .unwrap();

        assert_eq!(bundle.roots(), ["a", "a-b"]);
        assert_eq!(bundle.root(), None);
    }

    #[test]
    fn the_first_error_is_kept() {
        assert!(Bundle::builder()
            .module("a//b.js", "")
            .module("c.ts", "export const = ;")
            .build()
            .is_err_and(|error| {
                error.is_bundle() && error.to_string().contains("a//b.js")
            }));
    }

    #[test]
    fn two_paths_naming_one_module_are_rejected() {
        assert!(Bundle::builder()
            .module("a.js", "")
            .module("a.ts", "")
            .build()
            .is_err_and(|error| error.is_bundle()));
    }

    #[test]
    fn a_module_path_needs_a_script_extension() {
        assert!(Bundle::single("table.json", "{}")
            .is_err_and(|error| error.is_bundle()));
    }

    #[test]
    fn files_split_into_modules_and_data() {
        let bundle = Bundle::from_files([
            ("main.js", "export {}".as_bytes()),
            ("table.json", "{}".as_bytes()),
        ])
        .unwrap();

        assert!(bundle.contains("main"));
        assert!(!bundle.contains("table.json"));
        assert_eq!(bundle.data("table.json"), Some("{}".as_bytes()));
    }

    #[test]
    fn a_bundle_without_modules_is_rejected() {
        assert!(Bundle::builder()
            .data("table.json", "{}")
            .build()
            .is_err_and(|error| error.is_bundle()));
    }

    #[test]
    fn a_clone_retains_modules_and_data() {
        let bundle = Bundle::from_files([
            ("main.js", "export {}".as_bytes()),
            ("table.json", "{}".as_bytes()),
        ])
        .unwrap();
        let cloned = bundle.clone();

        assert_eq!(cloned.id(), bundle.id());

        drop(bundle);

        assert_eq!(cloned.module("main"), Some("export {}"));
        assert_eq!(cloned.data("table.json"), Some("{}".as_bytes()));
    }
}
