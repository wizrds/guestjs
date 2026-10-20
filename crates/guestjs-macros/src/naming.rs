use darling::FromMeta;
use heck::{
    ToKebabCase, ToLowerCamelCase, ToPascalCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
};
use syn::{Ident, ext::IdentExt};

#[derive(Clone, Copy)]
pub enum RenameRule {
    Lowercase,
    Uppercase,
    PascalCase,
    CamelCase,
    SnakeCase,
    ScreamingSnakeCase,
    KebabCase,
    ScreamingKebabCase,
}

impl RenameRule {
    const NAMES: [&str; 8] = [
        "lowercase",
        "UPPERCASE",
        "PascalCase",
        "camelCase",
        "snake_case",
        "SCREAMING_SNAKE_CASE",
        "kebab-case",
        "SCREAMING-KEBAB-CASE",
    ];

    pub fn apply(self, name: &str) -> String {
        match self {
            Self::Lowercase => name.to_lowercase(),
            Self::Uppercase => name.to_uppercase(),
            Self::PascalCase => name.to_pascal_case(),
            Self::CamelCase => name.to_lower_camel_case(),
            Self::SnakeCase => name.to_snake_case(),
            Self::ScreamingSnakeCase => name.to_shouty_snake_case(),
            Self::KebabCase => name.to_kebab_case(),
            Self::ScreamingKebabCase => name.to_shouty_kebab_case(),
        }
    }
}

impl FromMeta for RenameRule {
    fn from_string(value: &str) -> darling::Result<Self> {
        match value {
            "lowercase" => Ok(Self::Lowercase),
            "UPPERCASE" => Ok(Self::Uppercase),
            "PascalCase" => Ok(Self::PascalCase),
            "camelCase" => Ok(Self::CamelCase),
            "snake_case" => Ok(Self::SnakeCase),
            "SCREAMING_SNAKE_CASE" => Ok(Self::ScreamingSnakeCase),
            "kebab-case" => Ok(Self::KebabCase),
            "SCREAMING-KEBAB-CASE" => Ok(Self::ScreamingKebabCase),
            _ => Err(darling::Error::unknown_value_with_alts(value, &Self::NAMES)),
        }
    }
}

pub struct Naming;

impl Naming {
    pub fn member(ident: &Ident, name: Option<String>, rename_all: Option<RenameRule>) -> String {
        name.unwrap_or_else(|| match rename_all {
            Some(rule) => rule.apply(&ident.unraw().to_string()),
            None => ident.unraw().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use darling::FromMeta;
    use syn::parse_quote;

    use crate::naming::{Naming, RenameRule};

    #[test]
    fn supports_serde_rename_rules() {
        for (rule, expected) in [
            ("lowercase", "read_value"),
            ("UPPERCASE", "READ_VALUE"),
            ("PascalCase", "ReadValue"),
            ("camelCase", "readValue"),
            ("snake_case", "read_value"),
            ("SCREAMING_SNAKE_CASE", "READ_VALUE"),
            ("kebab-case", "read-value"),
            ("SCREAMING-KEBAB-CASE", "READ-VALUE"),
        ] {
            assert_eq!(
                RenameRule::from_string(rule)
                    .unwrap()
                    .apply("read_value"),
                expected,
            );
        }
    }

    #[test]
    fn rejects_unknown_rename_rules() {
        assert!(RenameRule::from_string("Title Case").is_err());
    }

    #[test]
    fn explicit_names_override_rename_rules() {
        assert_eq!(
            Naming::member(
                &parse_quote!(read_value),
                Some("send".to_owned()),
                Some(RenameRule::PascalCase),
            ),
            "send",
        );
    }

    #[test]
    fn raw_names_have_no_rust_prefix() {
        assert_eq!(Naming::member(&parse_quote!(r#type), None, None), "type",);
    }

    #[test]
    fn names_are_unchanged_without_a_rename_rule() {
        assert_eq!(Naming::member(&parse_quote!(read_value), None, None), "read_value",);
    }
}
