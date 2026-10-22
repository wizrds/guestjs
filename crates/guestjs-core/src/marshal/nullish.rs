use crate::{
    backend::{
        Val,
        values::{BackendValues, Kind},
    },
    enter::Enter,
    errors::Error,
    marshal::{
        FromGuest, ToGuest,
        describe::{Describe, Expected},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Nullish<T> {
    Undefined,
    Null,
    Some(T),
}

impl<T> Nullish<T> {
    pub fn map<U, F>(self, f: F) -> Nullish<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Self::Undefined => Nullish::Undefined,
            Self::Null => Nullish::Null,
            Self::Some(value) => Nullish::Some(f(value)),
        }
    }

    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Self::Some(value) => value,
            Self::Undefined | Self::Null => default,
        }
    }

    pub fn unwrap_or_else<F>(self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        match self {
            Self::Some(value) => value,
            Self::Undefined | Self::Null => f(),
        }
    }

    pub fn ok_or<E>(self, error: E) -> Result<T, E> {
        match self {
            Self::Some(value) => Ok(value),
            Self::Undefined | Self::Null => Err(error),
        }
    }

    pub fn ok_or_else<E, F>(self, f: F) -> Result<T, E>
    where
        F: FnOnce() -> E,
    {
        match self {
            Self::Some(value) => Ok(value),
            Self::Undefined | Self::Null => Err(f()),
        }
    }
}

impl<T> From<Nullish<T>> for Option<T> {
    fn from(value: Nullish<T>) -> Self {
        match value {
            Nullish::Some(value) => Some(value),
            Nullish::Undefined | Nullish::Null => None,
        }
    }
}

impl<T> From<Option<T>> for Nullish<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Some(value),
            None => Self::Undefined,
        }
    }
}

impl<T: Describe> Describe for Nullish<T> {
    fn describe(expected: &mut Expected) {
        T::describe(expected);
        expected.push("undefined");
        expected.push("null");
    }
}

impl<B, T> ToGuest<B> for Nullish<T>
where
    B: BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        match self {
            Self::Undefined => Ok(B::new_undefined(enter.token())),
            Self::Null => Ok(B::new_null(enter.token())),
            Self::Some(value) => value.to_guest(enter),
        }
    }
}

impl<B, T> FromGuest<B> for Nullish<T>
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Owned = Nullish<T::Owned>;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Undefined => Ok(Nullish::Undefined),
            Kind::Null => Ok(Nullish::Null),
            _ => T::from_guest(enter, value).map(Nullish::Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::marshal::{describe::Expected, nullish::Nullish};

    #[test]
    fn mapping_preserves_absence_and_transforms_values() {
        assert_eq!(Nullish::<i32>::Undefined.map(|value| value + 1), Nullish::Undefined,);
        assert_eq!(Nullish::<i32>::Null.map(|value| value + 1), Nullish::Null);
        assert_eq!(Nullish::Some(2).map(|value| value + 1), Nullish::Some(3));
    }

    #[test]
    fn option_conversion_is_explicitly_lossy() {
        assert_eq!(Option::<i32>::from(Nullish::Undefined), None);
        assert_eq!(Option::<i32>::from(Nullish::Null), None);
        assert_eq!(Option::from(Nullish::Some(3)), Some(3));
        assert_eq!(Nullish::<i32>::from(None), Nullish::Undefined);
        assert_eq!(Nullish::from(Some(3)), Nullish::Some(3));
    }

    #[test]
    fn fallback_functions_are_lazy() {
        let called = Cell::new(false);

        assert_eq!(
            Nullish::Some(3).unwrap_or_else(|| {
                called.set(true);

                4
            }),
            3,
        );
        assert!(!called.get());
        assert_eq!(Nullish::<i32>::Null.unwrap_or(4), 4);
        assert_eq!(Nullish::Some(3).ok_or("absent"), Ok(3));
        assert_eq!(Nullish::<i32>::Undefined.ok_or_else(|| "absent"), Err("absent"),);
    }

    #[test]
    fn description_includes_all_three_cases() {
        assert_eq!(Expected::of::<Nullish<String>>().to_string(), "string, undefined or null",);
    }
}
