use rquickjs::{Array, CatchResultExt, FromJs, IntoJs, Type, Value as JsValue, function::Args as JsArgs};

use crate::{errors::Error, runtime::Scope};

/// Converts a Rust value into a JavaScript value.
pub trait ToGuest {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error>;
}

/// Converts a Rust value into a JavaScript value within a scope.
pub trait ToGuestBound<'js> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error>;
}

/// Converts a JavaScript value into an owned Rust value.
pub trait FromGuest {
    /// The owned Rust value.
    type Owned: 'static;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error>;
}

/// Converts a JavaScript value into a scope-bound Rust value.
pub trait FromGuestBound {
    /// The scope-bound Rust value.
    type Bound<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error>;
}

/// Describes owned and scope-bound guest function inputs.
pub trait GuestType {
    /// The owned input type.
    type Owned: ToGuest;

    /// The scope-bound input type.
    type Bound<'js>: ToGuestBound<'js>;
}

impl<T> GuestType for T
where
    T: FromGuest + FromGuestBound,
    <T as FromGuest>::Owned: ToGuest,
    for<'js> <T as FromGuestBound>::Bound<'js>: ToGuestBound<'js>,
{
    type Owned = <T as FromGuest>::Owned;
    type Bound<'js> = <T as FromGuestBound>::Bound<'js>;
}

/// Converts a JavaScript value into a shared Rust borrow.
pub trait FromGuestRef<'js> {
    /// The shared borrow.
    type Ref;

    fn from_guest_ref(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Ref, Error>;
}

/// Converts a JavaScript value into an exclusive Rust borrow.
pub trait FromGuestMut<'js> {
    /// The exclusive borrow.
    type Mut;

    fn from_guest_mut(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Mut, Error>;
}

macro_rules! marshal_input {
    ($($value_type:ty),* $(,)?) => {
        $(
            impl ToGuest for $value_type {
                fn to_guest<'js>(
                    self,
                    scope: &Scope<'js>,
                ) -> Result<JsValue<'js>, Error> {
                    self.into_js(scope.ctx())
                        .catch(scope.ctx())
                        .map_err(Into::into)
                }
            }

            impl<'js> ToGuestBound<'js> for $value_type {
                fn to_guest_bound(
                    self,
                    scope: &Scope<'js>,
                ) -> Result<JsValue<'js>, Error> {
                    self.into_js(scope.ctx())
                        .catch(scope.ctx())
                        .map_err(Into::into)
                }
            }
        )*
    };
}

macro_rules! marshal_scalar {
    ($($value_type:ty),* $(,)?) => {
        marshal_input!($($value_type),*);

        $(
            impl FromGuest for $value_type {
                type Owned = Self;

                fn from_guest<'js>(
                    scope: &Scope<'js>,
                    value: JsValue<'js>,
                ) -> Result<Self::Owned, Error> {
                    <$value_type>::from_js(scope.ctx(), value)
                        .catch(scope.ctx())
                        .map_err(Into::into)
                }
            }

            impl FromGuestBound for $value_type {
                type Bound<'js> = Self;

                fn from_guest_bound<'js>(
                    scope: &Scope<'js>,
                    value: JsValue<'js>,
                ) -> Result<Self::Bound<'js>, Error> {
                    <$value_type>::from_js(scope.ctx(), value)
                        .catch(scope.ctx())
                        .map_err(Into::into)
                }
            }
        )*
    };
}

marshal_scalar!(
    (),
    bool,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    f32,
    f64,
    char,
    String,
);

marshal_input!(&str);

impl<T> ToGuest for Option<T>
where
    T: ToGuest,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        match self {
            Some(value) => value.to_guest(scope),
            None => Ok(JsValue::new_null(scope.ctx().clone())),
        }
    }
}

impl<'js, T> ToGuestBound<'js> for Option<T>
where
    T: ToGuestBound<'js>,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        match self {
            Some(value) => value.to_guest_bound(scope),
            None => Ok(JsValue::new_null(scope.ctx().clone())),
        }
    }
}

impl<T> FromGuest for Option<T>
where
    T: FromGuest,
{
    type Owned = Option<T::Owned>;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        match value.type_of() {
            Type::Undefined | Type::Uninitialized | Type::Null => Ok(None),
            _ => T::from_guest(scope, value).map(Some),
        }
    }
}

impl<T> FromGuestBound for Option<T>
where
    T: FromGuestBound,
{
    type Bound<'js> = Option<T::Bound<'js>>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        match value.type_of() {
            Type::Undefined | Type::Uninitialized | Type::Null => Ok(None),
            _ => T::from_guest_bound(scope, value).map(Some),
        }
    }
}

impl<T> ToGuest for Vec<T>
where
    T: ToGuest,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        let array = Array::new(scope.ctx().clone()).catch(scope.ctx())?;

        for (index, item) in self.into_iter().enumerate() {
            array
                .set(index, item.to_guest(scope)?)
                .catch(scope.ctx())?;
        }

        Ok(array.into_value())
    }
}

impl<'js, T> ToGuestBound<'js> for Vec<T>
where
    T: ToGuestBound<'js>,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        let array = Array::new(scope.ctx().clone()).catch(scope.ctx())?;

        for (index, item) in self.into_iter().enumerate() {
            array
                .set(index, item.to_guest_bound(scope)?)
                .catch(scope.ctx())?;
        }

        Ok(array.into_value())
    }
}

impl<T> FromGuest for Vec<T>
where
    T: FromGuest,
{
    type Owned = Vec<T::Owned>;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        let array = value
            .into_array()
            .ok_or_else(|| Error::conversion("expected an array"))?;

        let mut items = Vec::with_capacity(array.len());

        for index in 0..array.len() {
            items.push(T::from_guest(
                scope,
                array
                    .get::<JsValue>(index)
                    .catch(scope.ctx())?,
            )?);
        }

        Ok(items)
    }
}

impl<T> FromGuestBound for Vec<T>
where
    T: FromGuestBound,
{
    type Bound<'js> = Vec<T::Bound<'js>>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        let array = value
            .into_array()
            .ok_or_else(|| Error::conversion("expected an array"))?;

        let mut items = Vec::with_capacity(array.len());

        for index in 0..array.len() {
            items.push(T::from_guest_bound(
                scope,
                array
                    .get::<JsValue>(index)
                    .catch(scope.ctx())?,
            )?);
        }

        Ok(items)
    }
}

/// A JavaScript value that distinguishes `undefined` from `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nullish<T> {
    /// JavaScript `undefined`.
    Undefined,
    /// JavaScript `null`.
    Null,
    /// A present value.
    Some(T),
}

impl<T> Nullish<T> {
    /// Maps a present value.
    pub fn map<U, F>(self, f: F) -> Nullish<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Self::Some(value) => Nullish::Some(f(value)),
            Self::Null => Nullish::Null,
            Self::Undefined => Nullish::Undefined,
        }
    }

    /// Returns the present value or a default.
    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Self::Some(value) => value,
            _ => default,
        }
    }

    /// Returns the present value or computes a default.
    pub fn unwrap_or_else<F>(self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        match self {
            Self::Some(value) => value,
            _ => f(),
        }
    }

    /// Converts the present value into a result.
    pub fn ok_or<E>(self, error: E) -> Result<T, E> {
        match self {
            Self::Some(value) => Ok(value),
            _ => Err(error),
        }
    }

    /// Converts the present value into a result with a computed error.
    pub fn ok_or_else<E, F>(self, error: F) -> Result<T, E>
    where
        F: FnOnce() -> E,
    {
        match self {
            Self::Some(value) => Ok(value),
            _ => Err(error()),
        }
    }
}

impl<T> From<Nullish<T>> for Option<T> {
    fn from(value: Nullish<T>) -> Self {
        match value {
            Nullish::Some(value) => Some(value),
            _ => None,
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

impl<T> ToGuest for Nullish<T>
where
    T: ToGuest,
{
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        match self {
            Self::Some(value) => value.to_guest(scope),
            Self::Null => Ok(JsValue::new_null(scope.ctx().clone())),
            Self::Undefined => Ok(JsValue::new_undefined(scope.ctx().clone())),
        }
    }
}

impl<'js, T> ToGuestBound<'js> for Nullish<T>
where
    T: ToGuestBound<'js>,
{
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        match self {
            Self::Some(value) => value.to_guest_bound(scope),
            Self::Null => Ok(JsValue::new_null(scope.ctx().clone())),
            Self::Undefined => Ok(JsValue::new_undefined(scope.ctx().clone())),
        }
    }
}

impl<T> FromGuest for Nullish<T>
where
    T: FromGuest,
{
    type Owned = Nullish<T::Owned>;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        match value.type_of() {
            Type::Undefined | Type::Uninitialized => Ok(Nullish::Undefined),
            Type::Null => Ok(Nullish::Null),
            _ => T::from_guest(scope, value).map(Nullish::Some),
        }
    }
}

impl<T> FromGuestBound for Nullish<T>
where
    T: FromGuestBound,
{
    type Bound<'js> = Nullish<T::Bound<'js>>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        match value.type_of() {
            Type::Undefined | Type::Uninitialized => Ok(Nullish::Undefined),
            Type::Null => Ok(Nullish::Null),
            _ => T::from_guest_bound(scope, value).map(Nullish::Some),
        }
    }
}

/// Lowers a tuple of Rust values into a JavaScript call argument list.
pub trait ToGuestArgs {
    fn into_args<'js>(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error>;
}

/// Lowers scoped Rust values into a JavaScript call argument list.
pub trait ToGuestArgsBound<'js> {
    fn into_bound_args(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error>;
}

impl ToGuestArgs for () {
    fn into_args<'js>(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        Ok(JsArgs::new(scope.ctx().clone(), 0))
    }
}

impl<A> ToGuestArgs for (A,)
where
    A: ToGuest,
{
    fn into_args<'js>(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 1);

        args.push_arg(self.0.to_guest(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<A, B> ToGuestArgs for (A, B)
where
    A: ToGuest,
    B: ToGuest,
{
    fn into_args<'js>(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 2);

        args.push_arg(self.0.to_guest(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.1.to_guest(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<A, B, C> ToGuestArgs for (A, B, C)
where
    A: ToGuest,
    B: ToGuest,
    C: ToGuest,
{
    fn into_args<'js>(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 3);

        args.push_arg(self.0.to_guest(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.1.to_guest(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.2.to_guest(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<A, B, C, D> ToGuestArgs for (A, B, C, D)
where
    A: ToGuest,
    B: ToGuest,
    C: ToGuest,
    D: ToGuest,
{
    fn into_args<'js>(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 4);

        args.push_arg(self.0.to_guest(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.1.to_guest(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.2.to_guest(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.3.to_guest(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<'js> ToGuestArgsBound<'js> for () {
    fn into_bound_args(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        Ok(JsArgs::new(scope.ctx().clone(), 0))
    }
}

impl<'js, A> ToGuestArgsBound<'js> for (A,)
where
    A: ToGuestBound<'js>,
{
    fn into_bound_args(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 1);

        args.push_arg(self.0.to_guest_bound(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<'js, A, B> ToGuestArgsBound<'js> for (A, B)
where
    A: ToGuestBound<'js>,
    B: ToGuestBound<'js>,
{
    fn into_bound_args(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 2);

        args.push_arg(self.0.to_guest_bound(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.1.to_guest_bound(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<'js, A, B, C> ToGuestArgsBound<'js> for (A, B, C)
where
    A: ToGuestBound<'js>,
    B: ToGuestBound<'js>,
    C: ToGuestBound<'js>,
{
    fn into_bound_args(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 3);

        args.push_arg(self.0.to_guest_bound(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.1.to_guest_bound(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.2.to_guest_bound(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

impl<'js, A, B, C, D> ToGuestArgsBound<'js> for (A, B, C, D)
where
    A: ToGuestBound<'js>,
    B: ToGuestBound<'js>,
    C: ToGuestBound<'js>,
    D: ToGuestBound<'js>,
{
    fn into_bound_args(self, scope: &Scope<'js>) -> Result<JsArgs<'js>, Error> {
        let mut args = JsArgs::new(scope.ctx().clone(), 4);

        args.push_arg(self.0.to_guest_bound(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.1.to_guest_bound(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.2.to_guest_bound(scope)?)
            .catch(scope.ctx())?;
        args.push_arg(self.3.to_guest_bound(scope)?)
            .catch(scope.ctx())?;

        Ok(args)
    }
}

/// Marshals [`bytes::Bytes`](bytes::Bytes) to and from a JavaScript `Uint8Array`.
#[cfg(feature = "bytes")]
mod bytes_marshal {
    use bytes::Bytes;
    use rquickjs::{CatchResultExt, TypedArray, Value as JsValue};

    use crate::{
        errors::Error,
        marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
        runtime::Scope,
    };

    // `new_copy` uses QuickJS-owned storage, required to survive transfer and GC.
    impl ToGuest for Bytes {
        fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
            Ok(TypedArray::<u8>::new_copy(scope.ctx().clone(), self.as_ref())
                .catch(scope.ctx())?
                .into_value())
        }
    }

    impl<'js> ToGuestBound<'js> for Bytes {
        fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
            self.to_guest(scope)
        }
    }

    impl FromGuest for Bytes {
        type Owned = Self;

        fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
            // Copy out: the view borrows QuickJS storage tied to the scope.
            Ok(Bytes::copy_from_slice(
                TypedArray::<u8>::from_value(value)
                    .catch(scope.ctx())?
                    .as_bytes()
                    .ok_or_else(|| Error::conversion("Uint8Array is detached"))?,
            ))
        }
    }

    impl FromGuestBound for Bytes {
        type Bound<'js> = Self;

        fn from_guest_bound<'js>(
            scope: &Scope<'js>,
            value: JsValue<'js>,
        ) -> Result<Self::Bound<'js>, Error> {
            Self::from_guest(scope, value)
        }
    }
}

#[cfg(all(test, feature = "bytes"))]
mod bytes_tests {
    use bytes::Bytes;

    use crate::runtime::Runtime;

    #[tokio::test]
    async fn bytes_round_trip_through_uint8array() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        // A guest Uint8Array deserializes into Bytes preserving order and length.
        assert_eq!(
            guest
                .eval::<Bytes>("new Uint8Array([1, 2, 3, 255])")
                .await
                .unwrap(),
            Bytes::from_static(&[1, 2, 3, 255]),
        );

        // A host Bytes serializes into a guest Uint8Array the guest can measure and index.
        assert_eq!(
            guest
                .guest_module(
                    "echo.js",
                    "export function describe(view) { return `${view.constructor.name}:${view.length}:${view[3]}`; }",
                )
                .await
                .unwrap()
                .function("describe")
                .await
                .unwrap()
                .call::<_, String>((Bytes::from_static(&[9, 8, 7, 42]),))
                .await
                .unwrap(),
            "Uint8Array:4:42",
        );
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::{CatchResultExt, Function as JsFunction, Type, Value as JsValue};

    use crate::{
        errors::Error,
        handle::{Class, Function, Instance, Object, Promise},
        marshal::{
            FromGuest, FromGuestBound, GuestType, Nullish, ToGuest, ToGuestArgsBound, ToGuestBound,
        },
        runtime::{Runtime, Scope},
    };

    struct GuestTypeContract;

    impl GuestTypeContract {
        fn accepts<T>()
        where
            T: GuestType,
        {
        }
    }

    struct InputOnly;

    impl GuestType for InputOnly {
        type Owned = i32;
        type Bound<'js> = i32;
    }

    #[derive(Debug, PartialEq, Eq)]
    struct OwnedOnly(i32);

    impl FromGuest for OwnedOnly {
        type Owned = Self;

        fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
            Ok(Self(i32::from_guest(scope, value)?))
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct BoundOnly(i32);

    impl FromGuestBound for BoundOnly {
        type Bound<'js> = Self;

        fn from_guest_bound<'js>(
            scope: &Scope<'js>,
            value: JsValue<'js>,
        ) -> Result<Self::Bound<'js>, Error> {
            Ok(Self(i32::from_guest_bound(scope, value)?))
        }
    }

    #[test]
    fn guest_type_projects_existing_descriptors() {
        GuestTypeContract::accepts::<i32>();
        GuestTypeContract::accepts::<Vec<i32>>();
        GuestTypeContract::accepts::<Option<Function>>();
        GuestTypeContract::accepts::<Nullish<Object>>();
        GuestTypeContract::accepts::<Function>();
        GuestTypeContract::accepts::<Object>();
        GuestTypeContract::accepts::<Class>();
        GuestTypeContract::accepts::<Instance>();
        GuestTypeContract::accepts::<Promise<Function>>();
        GuestTypeContract::accepts::<InputOnly>();
    }

    #[tokio::test]
    async fn scalar_and_container_roundtrip() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        assert_eq!(
            guest
                .eval::<i32>("1 + 2")
                .await
                .unwrap(),
            3
        );
        assert_eq!(
            guest
                .eval::<Vec<i32>>("[1, 2, 3]")
                .await
                .unwrap(),
            vec![1, 2, 3],
        );
        assert_eq!(
            guest
                .eval::<Option<i32>>("undefined")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            guest
                .eval::<Option<i32>>("null")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            guest
                .eval::<Option<i32>>("7")
                .await
                .unwrap(),
            Some(7)
        );
    }

    #[tokio::test]
    async fn nullish_distinguishes_undefined_from_null() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        assert!(matches!(
            guest
                .eval::<Nullish<i32>>("undefined")
                .await
                .unwrap(),
            Nullish::Undefined,
        ));
        assert!(matches!(
            guest
                .eval::<Nullish<i32>>("null")
                .await
                .unwrap(),
            Nullish::Null,
        ));
        assert!(matches!(
            guest
                .eval::<Nullish<i32>>("42")
                .await
                .unwrap(),
            Nullish::Some(42),
        ));
    }

    #[tokio::test]
    async fn outbound_optional_values_use_null() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async |scope| {
                assert_eq!(None::<i32>.to_guest(&scope)?.type_of(), Type::Null);
                assert_eq!(
                    None::<i32>
                        .to_guest_bound(&scope)?
                        .type_of(),
                    Type::Null,
                );
                assert_eq!(i32::from_guest_bound(&scope, Some(42).to_guest(&scope)?,)?, 42,);
                assert_eq!(i32::from_guest_bound(&scope, Some(42).to_guest_bound(&scope)?,)?, 42,);
                assert_eq!(
                    Nullish::<i32>::Undefined
                        .to_guest(&scope)?
                        .type_of(),
                    Type::Undefined,
                );
                assert_eq!(
                    Nullish::<i32>::Undefined
                        .to_guest_bound(&scope)?
                        .type_of(),
                    Type::Undefined,
                );

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn owned_conversion_does_not_require_bound_conversion() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<OwnedOnly>("40 + 2")
                .await
                .unwrap(),
            OwnedOnly(42),
        );
    }

    #[tokio::test]
    async fn bound_conversion_does_not_require_owned_conversion() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .scope(async |scope| {
                    BoundOnly::from_guest_bound(
                        &scope,
                        scope
                            .ctx()
                            .eval::<JsValue, _>("6 * 7")
                            .catch(scope.ctx())?,
                    )
                })
                .await
                .unwrap(),
            BoundOnly(42),
        );
    }

    #[tokio::test]
    async fn scoped_values_convert_recursively() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async |scope| {
                assert_eq!(
                    Vec::<Nullish<i32>>::from_guest_bound(
                        &scope,
                        scope
                            .ctx()
                            .eval::<JsValue, _>("[1, null, undefined]")
                            .catch(scope.ctx())?,
                    )?,
                    vec![Nullish::Some(1), Nullish::Null, Nullish::Undefined,],
                );
                assert_eq!(
                    Vec::<Option<i32>>::from_guest_bound(
                        &scope,
                        vec![Some(2), None, Some(4)].to_guest_bound(&scope)?,
                    )?,
                    vec![Some(2), None, Some(4)],
                );
                assert_eq!(
                    i32::from_guest_bound(
                        &scope,
                        scope
                            .ctx()
                            .eval::<JsFunction, _>("(a, b, c, d) => a + b + c + d",)
                            .catch(scope.ctx())?
                            .call_arg::<JsValue>((1, 2, 3, 4).into_bound_args(&scope)?,)
                            .catch(scope.ctx())?,
                    )?,
                    10,
                );

                Ok(())
            })
            .await
            .unwrap();
    }
}
