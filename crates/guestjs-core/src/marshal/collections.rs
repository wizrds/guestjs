use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
        HashSet,
    },
    hash::Hash,
    iter::from_fn,
    marker::PhantomData,
    vec::IntoIter,
};

use crate::{
    backend::{
        values::{
            BackendValues,
            IteratorKind,
            Kind,
        },
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        ToGuest,
        ToGuestArgs,
    },
};

pub struct Elements<'js, B: Backend> {
    values: Vec<Val<'js, B>>,
}

impl<'js, B: BackendValues> Elements<'js, B> {
    pub fn read(
        enter: &Enter<'js, B>,
        array: &Val<'js, B>,
    ) -> Result<Self, Error> {
        match B::kind(enter.token(), array) {
            Kind::Array => Ok(Self {
                values: (0..usize::from_guest(
                    enter,
                    B::get(
                        enter.token(),
                        array,
                        &"length".to_guest(enter)?,
                    )?,
                )?)
                    .map(|index| {
                        index
                            .to_guest(enter)
                            .and_then(|key| B::get(enter.token(), array, &key))
                    })
                    .collect::<Result<_, _>>()?,
            }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }

    pub fn exact<const N: usize>(self) -> Result<[Val<'js, B>; N], Error> {
        <[_; N]>::try_from(self.values).map_err(|values| {
            Error::conversion(
                format!("array of length {N}"),
                format!("array of length {}", values.len()),
            )
        })
    }
}

impl<B: Backend> Describe for Elements<'_, B> {
    fn describe(expected: &mut Expected) {
        expected.push("array");
    }
}

impl<'js, B: Backend> IntoIterator for Elements<'js, B> {
    type Item = Val<'js, B>;
    type IntoIter = IntoIter<Val<'js, B>>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

pub struct Entries<'js, B: Backend> {
    entries: Vec<(Val<'js, B>, Val<'js, B>)>,
}

impl<'js, B: BackendValues> Entries<'js, B> {
    pub fn read(
        enter: &Enter<'js, B>,
        object: &Val<'js, B>,
    ) -> Result<Self, Error> {
        match B::kind(enter.token(), object) {
            Kind::Object => Ok(Self {
                entries: B::keys(enter.token(), object)?
                    .into_iter()
                    .map(|key| {
                        B::get(enter.token(), object, &key)
                            .map(|value| (key, value))
                    })
                    .collect::<Result<_, _>>()?,
            }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }

    pub fn exact<const N: usize>(
        self,
    ) -> Result<[(Val<'js, B>, Val<'js, B>); N], Error> {
        <[_; N]>::try_from(self.entries).map_err(|entries| {
            Error::conversion(
                format!("object with {N} properties"),
                format!("object with {} properties", entries.len()),
            )
        })
    }
}

impl<B: Backend> Describe for Entries<'_, B> {
    fn describe(expected: &mut Expected) {
        expected.push("object");
    }
}

impl<'js, B: Backend> IntoIterator for Entries<'js, B> {
    type Item = (Val<'js, B>, Val<'js, B>);
    type IntoIter = IntoIter<(Val<'js, B>, Val<'js, B>)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

pub struct Members<'js, B: Backend> {
    values: Vec<Val<'js, B>>,
}

impl<'js, B: BackendValues> Members<'js, B> {
    pub fn read(
        enter: &Enter<'js, B>,
        set: &Val<'js, B>,
    ) -> Result<Self, Error> {
        match B::kind(enter.token(), set) {
            Kind::Set => match B::iter(enter.token(), set, IteratorKind::Sync)? {
                Some(iterator) => Ok(Self {
                    values: from_fn(|| {
                        B::next(enter.token(), &iterator)
                            .and_then(|result| B::step(enter.token(), &result))
                            .transpose()
                    })
                    .collect::<Result<_, _>>()?,
                }),
                None => Err(Expected::of::<Self>().mismatch(Kind::Set)),
            },
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> Describe for Members<'_, B> {
    fn describe(expected: &mut Expected) {
        expected.push("set");
    }
}

impl<'js, B: Backend> IntoIterator for Members<'js, B> {
    type Item = Val<'js, B>;
    type IntoIter = IntoIter<Val<'js, B>>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

pub struct IterItem<T> {
    target: PhantomData<fn() -> T>,
}

impl<T> Describe for IterItem<T> {
    fn describe(expected: &mut Expected) {
        expected.push("iter item");
    }
}

impl<B, T> FromGuest<B> for IterItem<T>
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Owned = Option<T::Owned>;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            kind if kind.is_object() => B::step(enter.token(), &value)?
                .map(|item| T::from_guest(enter, item))
                .transpose(),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

macro_rules! tuple {
    ($count:literal; $($name:ident $value:ident),+) => {
        impl<$($name),+> Describe for ($($name,)+) {
            fn describe(expected: &mut Expected) {
                expected.push("array");
            }
        }

        impl<B, $($name),+> ToGuest<B> for ($($name,)+)
        where
            B: BackendValues,
            $($name: ToGuest<B>,)+
        {
            fn to_guest<'js>(
                self,
                enter: &Enter<'js, B>,
            ) -> Result<Val<'js, B>, Error> {
                B::new_array(enter.token(), self.to_guest_args(enter)?)
            }
        }

        impl<B, $($name),+> FromGuest<B> for ($($name,)+)
        where
            B: BackendValues,
            $($name: FromGuest<B>,)+
        {
            type Owned = ($($name::Owned,)+);

            fn from_guest<'js>(
                enter: &Enter<'js, B>,
                value: Val<'js, B>,
            ) -> Result<Self::Owned, Error> {
                let [$($value),+] = Elements::read(enter, &value)?
                    .exact::<$count>()?;

                Ok(($($name::from_guest(enter, $value)?,)+))
            }
        }
    };
}

macro_rules! map {
    ($map:ident, $($key_bound:tt)+) => {
        impl<K, V> Describe for $map<K, V> {
            fn describe(expected: &mut Expected) {
                expected.push("object");
            }
        }

        impl<B, K, V> ToGuest<B> for $map<K, V>
        where
            B: BackendValues,
            K: ToGuest<B>,
            V: ToGuest<B>,
        {
            fn to_guest<'js>(
                self,
                enter: &Enter<'js, B>,
            ) -> Result<Val<'js, B>, Error> {
                self.into_iter().try_fold(
                    B::new_object(enter.token())?,
                    |object, (key, value)| {
                        B::set(
                            enter.token(),
                            &object,
                            &key.to_guest(enter)?,
                            value.to_guest(enter)?,
                        )
                        .map(|()| object)
                    },
                )
            }
        }

        impl<B, K, V> FromGuest<B> for $map<K, V>
        where
            B: BackendValues,
            K: FromGuest<B>,
            K::Owned: $($key_bound)+,
            V: FromGuest<B>,
        {
            type Owned = $map<K::Owned, V::Owned>;

            fn from_guest<'js>(
                enter: &Enter<'js, B>,
                value: Val<'js, B>,
            ) -> Result<Self::Owned, Error> {
                Entries::read(enter, &value)?
                    .into_iter()
                    .map(|(key, item)| {
                        K::from_guest(enter, key).and_then(|key| {
                            V::from_guest(enter, item).map(|item| (key, item))
                        })
                    })
                    .collect()
            }
        }
    };
}

macro_rules! set {
    ($set:ident, $($item_bound:tt)+) => {
        impl<T> Describe for $set<T> {
            fn describe(expected: &mut Expected) {
                expected.push("set");
            }
        }

        impl<B, T> ToGuest<B> for $set<T>
        where
            B: BackendValues,
            T: ToGuest<B>,
        {
            fn to_guest<'js>(
                self,
                enter: &Enter<'js, B>,
            ) -> Result<Val<'js, B>, Error> {
                B::new_set(
                    enter.token(),
                    self.into_iter()
                        .collect::<Vec<_>>()
                        .to_guest_args(enter)?,
                )
            }
        }

        impl<B, T> FromGuest<B> for $set<T>
        where
            B: BackendValues,
            T: FromGuest<B>,
            T::Owned: $($item_bound)+,
        {
            type Owned = $set<T::Owned>;

            fn from_guest<'js>(
                enter: &Enter<'js, B>,
                value: Val<'js, B>,
            ) -> Result<Self::Owned, Error> {
                Members::read(enter, &value)?
                    .into_iter()
                    .map(|item| T::from_guest(enter, item))
                    .collect()
            }
        }
    };
}

tuple!(1; A a);
tuple!(2; A a, B2 b);
tuple!(3; A a, B2 b, C c);
tuple!(4; A a, B2 b, C c, D d);
tuple!(5; A a, B2 b, C c, D d, E e);
tuple!(6; A a, B2 b, C c, D d, E e, F f);
tuple!(7; A a, B2 b, C c, D d, E e, F f, G g);
tuple!(8; A a, B2 b, C c, D d, E e, F f, G g, H h);
tuple!(9; A a, B2 b, C c, D d, E e, F f, G g, H h, I i);
tuple!(10; A a, B2 b, C c, D d, E e, F f, G g, H h, I i, J j);
tuple!(11; A a, B2 b, C c, D d, E e, F f, G g, H h, I i, J j, K k);
tuple!(12; A a, B2 b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);
map!(HashMap, Eq + Hash);
map!(BTreeMap, Ord);
set!(HashSet, Eq + Hash);
set!(BTreeSet, Ord);

impl<T: Describe> Describe for Option<T> {
    fn describe(expected: &mut Expected) {
        T::describe(expected);
        expected.push("undefined");
        expected.push("null");
    }
}

impl<B, T> ToGuest<B> for Option<T>
where
    B: BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        match self {
            Some(value) => value.to_guest(enter),
            None => Ok(B::new_null(enter.token())),
        }
    }
}

impl<B, T> FromGuest<B> for Option<T>
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Owned = Option<T::Owned>;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Undefined | Kind::Null => Ok(None),
            _ => T::from_guest(enter, value).map(Some),
        }
    }
}

impl<T> Describe for Vec<T> {
    fn describe(expected: &mut Expected) {
        expected.push("array");
    }
}

impl<B, T> ToGuest<B> for Vec<T>
where
    B: BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        B::new_array(enter.token(), self.to_guest_args(enter)?)
    }
}

impl<B, T> FromGuest<B> for Vec<T>
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Owned = Vec<T::Owned>;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        Elements::read(enter, &value)?
            .into_iter()
            .map(|item| T::from_guest(enter, item))
            .collect()
    }
}

impl<T, const N: usize> Describe for [T; N] {
    fn describe(expected: &mut Expected) {
        expected.push("array");
    }
}

impl<B, T, const N: usize> ToGuest<B> for [T; N]
where
    B: BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        Vec::from(self).to_guest(enter)
    }
}

impl<B, T, const N: usize> FromGuest<B> for [T; N]
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Owned = [T::Owned; N];

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        Vec::<T>::from_guest(enter, value)?
            .try_into()
            .map_err(|values: Vec<_>| {
                Error::conversion(
                    format!("array of length {N}"),
                    format!("array of length {}", values.len()),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{
        BTreeSet,
        HashMap,
        HashSet,
    };

    use crate::marshal::{
        collections::IterItem,
        describe::Expected,
    };

    #[test]
    fn option_adds_the_nullish_values() {
        assert_eq!(
            Expected::of::<Option<bool>>().to_string(),
            "boolean, undefined or null",
        );
        assert_eq!(
            Expected::of::<Option<Option<bool>>>().to_string(),
            "boolean, undefined or null",
        );
    }

    #[test]
    fn sequences_accept_arrays() {
        assert_eq!(Expected::of::<Vec<i64>>().to_string(), "array");
        assert_eq!(Expected::of::<(i64, String)>().to_string(), "array");
    }

    #[test]
    fn fixed_arrays_accept_arrays() {
        assert_eq!(Expected::of::<[i64; 3]>().to_string(), "array");
    }

    #[test]
    fn sets_accept_sets() {
        assert_eq!(Expected::of::<HashSet<i64>>().to_string(), "set");
        assert_eq!(Expected::of::<BTreeSet<i64>>().to_string(), "set");
    }


    #[test]
    fn item_description_names_an_iter_item() {
        assert_eq!(
            Expected::of::<IterItem<String>>().to_string(),
            "iter item",
        );
    }

    #[test]
    fn maps_accept_objects() {
        assert_eq!(
            Expected::of::<HashMap<String, i64>>().to_string(),
            "object",
        );
    }
}
