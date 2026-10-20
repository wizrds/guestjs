use std::borrow::Cow;

use crate::{
    backend::values::Kind,
    errors::Error,
};

pub trait Describe {
    fn describe(expected: &mut Expected);
}

pub struct Expected {
    names: Vec<Cow<'static, str>>,
}

impl Expected {
    pub fn of<T>() -> Self
    where
        T: Describe + ?Sized,
    {
        let mut expected = Self {
            names: Vec::new(),
        };

        T::describe(&mut expected);

        expected
    }

    pub fn push<N>(&mut self, name: N)
    where
        N: Into<Cow<'static, str>>,
    {
        let name = name.into();

        if !self.names.contains(&name) {
            self.names.push(name);
        }
    }

    pub fn mismatch(&self, found: Kind) -> Error {
        Error::conversion(
            self.to_string(),
            match found {
                Kind::Undefined => "undefined",
                Kind::Null => "null",
                Kind::Boolean => "boolean",
                Kind::Number => "number",
                Kind::BigInt => "bigint",
                Kind::String => "string",
                Kind::Symbol => "symbol",
                Kind::Array => "array",
                Kind::Function => "function",
                Kind::Promise => "promise",
                Kind::ArrayBuffer => "ArrayBuffer",
                Kind::TypedArray => "typed array",
                Kind::DataView => "DataView",
                Kind::Set => "Set",
                Kind::Object => "object",
            },
        )
    }
}

impl std::fmt::Display for Expected {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self.names.split_last() {
            None => formatter.write_str("nothing"),
            Some((last, [])) => formatter.write_str(last),
            Some((last, rest)) => {
                write!(formatter, "{} or {last}", rest.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::values::Kind,
        marshal::describe::{
            Describe,
            Expected,
        },
    };

    struct Empty;

    impl Describe for Empty {
        fn describe(_expected: &mut Expected) {}
    }

    struct Names;

    impl Describe for Names {
        fn describe(expected: &mut Expected) {
            expected.push("first");
            expected.push("second");
            expected.push("second");
            expected.push("third");
        }
    }

    #[test]
    fn expected_renders_an_empty_description() {
        assert_eq!(Expected::of::<Empty>().to_string(), "nothing");
    }

    #[test]
    fn expected_preserves_order_and_removes_duplicate_names() {
        assert_eq!(
            Expected::of::<Names>().to_string(),
            "first, second or third",
        );
    }

    #[test]
    fn mismatch_is_a_conversion_naming_the_found_kind() {
        assert!(Expected::of::<Names>()
            .mismatch(Kind::TypedArray)
            .is_conversion());
        assert_eq!(
            Expected::of::<Names>()
                .mismatch(Kind::TypedArray)
                .to_string(),
            "expected first, second or third, found typed array",
        );
    }
}
