use crate::{
    bundle::name::ModuleName,
    errors::Error,
};

pub struct Resolver;

impl Resolver {
    pub fn new() -> Self {
        Self
    }

    fn join(
        &self,
        referrer: &str,
        specifier: &str,
    ) -> Result<String, Error> {
        if referrer.is_empty() {
            return Err(Error::import(
                specifier,
                "a relative specifier needs a referring module",
            ));
        }

        let joined = specifier
            .split('/')
            .try_fold(
                referrer
                    .rsplit_once('/')
                    .map(|(directory, _)| {
                        directory.split('/').collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                |mut segments, segment| match segment {
                    "." => Ok(segments),
                    ".." => segments
                        .pop()
                        .map(|_| segments)
                        .ok_or_else(|| {
                            Error::import(
                                specifier,
                                "the specifier leaves the root of its module tree",
                            )
                        }),
                    "" => Err(Error::import(
                        specifier,
                        "the specifier has an empty path segment",
                    )),
                    segment => {
                        segments.push(segment);

                        Ok(segments)
                    },
                },
            )?
            .join("/");

        Ok(ModuleName::from_path(&joined)
            .map(|name| name.as_str())
            .unwrap_or(joined.as_str())
            .to_owned())
    }

    pub fn resolve(
        &self,
        referrer: &str,
        specifier: &str,
    ) -> Result<String, Error> {
        match specifier {
            "" => Err(Error::import(
                specifier,
                "an empty specifier names no module",
            )),
            _ if specifier.starts_with('/') => {
                Err(Error::import(
                    specifier,
                    "absolute specifiers are not supported",
                ))
            },
            _ if specifier.starts_with("./")
                || specifier.starts_with("../") =>
            {
                self.join(referrer, specifier)
            },
            _ => Ok(specifier.to_owned()),
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::imports::resolver::Resolver;

    #[test]
    fn bare_specifiers_are_names() {
        assert_eq!(
            Resolver::new()
                .resolve("plugin/index", "geometry")
                .unwrap(),
            "geometry",
        );
        assert_eq!(
            Resolver::new()
                .resolve("", "@scope/pkg/sub")
                .unwrap(),
            "@scope/pkg/sub",
        );
    }

    #[test]
    fn relative_specifiers_join_the_referrer_directory() {
        assert_eq!(
            Resolver::new()
                .resolve("plugin/index", "./util")
                .unwrap(),
            "plugin/util",
        );
        assert_eq!(
            Resolver::new()
                .resolve("plugin/a/b", "../c")
                .unwrap(),
            "plugin/c",
        );
        assert_eq!(
            Resolver::new()
                .resolve("plugin/index", "../x")
                .unwrap(),
            "x",
        );
    }

    #[test]
    fn script_extensions_are_dropped() {
        assert_eq!(
            Resolver::new()
                .resolve("plugin/index", "./util.js")
                .unwrap(),
            "plugin/util",
        );
        assert_eq!(
            Resolver::new()
                .resolve("plugin/index", "./util.ts")
                .unwrap(),
            "plugin/util",
        );
        assert_eq!(
            Resolver::new().resolve("plugin/index", "./table.json").unwrap(),
            "plugin/table.json",
        );
    }

    #[test]
    fn leaving_the_root_is_rejected() {
        assert!(Resolver::new()
            .resolve("main", "../x")
            .is_err_and(|error| error.is_import()));
    }

    #[test]
    fn a_relative_specifier_needs_a_referrer() {
        assert!(Resolver::new()
            .resolve("", "./x")
            .is_err_and(|error| error.is_import()));
    }

    #[test]
    fn absolute_empty_and_gapped_specifiers_are_rejected() {
        ["/x", "", "./a//b"]
            .into_iter()
            .for_each(|specifier| {
                assert!(
                    Resolver::new()
                        .resolve("main", specifier)
                        .is_err_and(|error| error.is_import()),
                    "{specifier}",
                );
            });
    }
}
