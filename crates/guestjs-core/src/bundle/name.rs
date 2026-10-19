#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleName<'a> {
    name: &'a str,
}

impl<'a> ModuleName<'a> {
    const SCRIPT_EXTENSIONS: [&'static str; 4] = [".js", ".mjs", ".ts", ".mts"];

    pub fn new(name: &'a str) -> Self {
        Self {
            name,
        }
    }

    pub fn from_path(path: &'a str) -> Option<Self> {
        Self::SCRIPT_EXTENSIONS
            .iter()
            .find_map(|extension| path.strip_suffix(extension))
            .filter(|name| !name.is_empty() && !name.ends_with('/'))
            .map(Self::new)
    }

    pub fn as_str(&self) -> &'a str {
        self.name
    }

    pub fn root(&self) -> &'a str {
        self.name
            .split('/')
            .next()
            .unwrap_or(self.name)
    }

    pub fn is_within(&self, prefix: &str) -> bool {
        self.name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }
}

#[cfg(test)]
mod tests {
    use crate::bundle::name::ModuleName;

    #[test]
    fn the_root_is_the_first_segment() {
        assert_eq!(ModuleName::new("plugin/util").root(), "plugin");
        assert_eq!(ModuleName::new("geometry").root(), "geometry");
    }

    #[test]
    fn a_name_is_within_itself_and_its_ancestors() {
        assert!(ModuleName::new("fs").is_within("fs"));
        assert!(ModuleName::new("fs/promises").is_within("fs"));
        assert!(!ModuleName::new("fsx").is_within("fs"));
        assert!(!ModuleName::new("fs").is_within("fs/promises"));
    }

    #[test]
    fn a_script_path_declares_its_name_without_the_extension() {
        assert_eq!(
            ModuleName::from_path("plugin/util.ts").map(|name| name.as_str()),
            Some("plugin/util"),
        );
        assert_eq!(
            ModuleName::from_path("main.mjs").map(|name| name.as_str()),
            Some("main"),
        );
    }

    #[test]
    fn other_paths_declare_no_name() {
        assert_eq!(ModuleName::from_path("table.json"), None);
        assert_eq!(ModuleName::from_path(".js"), None);
        assert_eq!(ModuleName::from_path("plugin/.ts"), None);
    }
}
