use std::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
};

use rquickjs::{CatchResultExt, Persistent, Promise as JsPromise, Value as JsValue};

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest value awaited outside a scope.
pub struct Awaitable<T> {
    value: Persistent<JsValue<'static>>,
    context: Rc<GuestContext>,
    _result: PhantomData<fn() -> T>,
}

impl<T> Awaitable<T> {
    fn new(value: Persistent<JsValue<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context, _result: PhantomData }
    }

    /// Binds the awaitable value to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundAwaitable<'js, T>, Error> {
        Ok(BoundAwaitable::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }
}

impl<T: 'static> FromGuest for Awaitable<T> {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Self::new(
            Persistent::save(scope.ctx(), value),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<T> FromGuestBound for Awaitable<T> {
    type Bound<'js> = BoundAwaitable<'js, T>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundAwaitable::new(value, scope.clone()))
    }
}

impl<T> ToGuest for Awaitable<T> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.value
            .restore(scope.ctx())
            .catch(scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js, T> ToGuestBound<'js> for Awaitable<T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl<T> IntoFuture for Awaitable<T>
where
    T: FromGuest + 'static,
{
    type Output = Result<T::Owned, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            Scope::with(&self.context, async |scope| {
                T::from_guest(
                    &scope,
                    self.bind(&scope)?
                        .resolve_value()
                        .await?,
                )
            })
            .await
        })
    }
}

/// A guest value awaited within its scope.
pub struct BoundAwaitable<'js, T> {
    value: JsValue<'js>,
    scope: Scope<'js>,
    _result: PhantomData<fn() -> T>,
}

impl<'js, T> BoundAwaitable<'js, T> {
    fn new(value: JsValue<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope, _result: PhantomData }
    }

    async fn resolve_value(self) -> Result<JsValue<'js>, Error> {
        if !self.value.is_promise() {
            return Ok(self.value);
        }

        JsPromise::from_value(self.value)
            .map_err(|error| Error::sourced_conversion("expected a promise", Some(error)))?
            .into_future()
            .await
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    /// Converts the awaitable value into an owned handle.
    pub fn into_owned(self) -> Result<Awaitable<T>, Error> {
        Ok(Awaitable::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<'js, T> IntoFuture for BoundAwaitable<'js, T>
where
    T: FromGuestBound + 'js,
{
    type Output = Result<T::Bound<'js>, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + 'js>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(
            async move { T::from_guest_bound(&self.scope.clone(), self.resolve_value().await?) },
        )
    }
}

impl<'js, T> ToGuestBound<'js> for BoundAwaitable<'js, T> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        __private,
        errors::Error,
        handle::Awaitable,
        marshal::{FromGuest, FromGuestBound},
        runtime::{Runtime, Scope},
        __private::JsValue,
    };

    const AWAITABLE_SOURCE: &str = r#"
        export const directScalar = 42;
        export const promisedScalar = Promise.resolve(42);
        export const directObject = {
            name: "direct",
            count: 2,
        };
        export const promisedObject = Promise.resolve({
            name: "promised",
            count: 3,
        });
        export const rejected = Promise.reject(new Error("awaitable failed"));
        export const directInvalid = "invalid";
        export const promisedInvalid = Promise.resolve("invalid");
    "#;

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Record {
        name: String,
        count: u32,
    }

    impl FromGuest for Record {
        type Owned = Self;

        fn from_guest<'js>(_scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
            __private::from_value(value)
        }
    }

    impl FromGuestBound for Record {
        type Bound<'js> = Self;

        fn from_guest_bound<'js>(
            _scope: &Scope<'js>,
            value: JsValue<'js>,
        ) -> Result<Self::Bound<'js>, Error> {
            __private::from_value(value)
        }
    }

    #[tokio::test]
    async fn owned_awaitable_converts_direct_value() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<Awaitable<i32>>("42")
                .await
                .unwrap()
                .await
                .unwrap(),
            42,
        );
    }

    #[tokio::test]
    async fn owned_awaitable_resolves_promise() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<Awaitable<i32>>("Promise.resolve(42)")
                .await
                .unwrap()
                .await
                .unwrap(),
            42,
        );
    }

    #[tokio::test]
    async fn bound_awaitable_converts_direct_object() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("direct-object.js", AWAITABLE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            guest
                .scope(async move |scope| {
                    module
                        .bind(&scope)?
                        .get::<Awaitable<Record>>("directObject")?
                        .await
                })
                .await
                .unwrap(),
            Record { name: "direct".to_owned(), count: 2 },
        );
    }

    #[tokio::test]
    async fn bound_awaitable_resolves_object_promise() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("promised-object.js", AWAITABLE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            guest
                .scope(async move |scope| {
                    module
                        .bind(&scope)?
                        .get::<Awaitable<Record>>("promisedObject")?
                        .await
                })
                .await
                .unwrap(),
            Record { name: "promised".to_owned(), count: 3 },
        );
    }

    #[tokio::test]
    async fn promoted_awaitable_survives_original_scope() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("promoted-awaitable.js", AWAITABLE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            guest
                .scope(async move |scope| {
                    module
                        .bind(&scope)?
                        .get::<Awaitable<i32>>("promisedScalar")?
                        .into_owned()
                })
                .await
                .unwrap()
                .await
                .unwrap(),
            42,
        );
    }

    #[tokio::test]
    async fn rejected_awaitable_preserves_guest_exception() {
        match Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module("rejected-awaitable.js", AWAITABLE_SOURCE)
            .await
            .unwrap()
            .get::<Awaitable<i32>>("rejected")
            .await
            .unwrap()
            .await
            .unwrap_err()
        {
            Error::GuestException { message, .. } => {
                assert!(message.contains("awaitable failed"));
            }
            error => panic!("expected guest exception, got {error}"),
        }
    }

    #[tokio::test]
    async fn direct_and_promised_values_share_conversion_error() {
        let module = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module("invalid-awaitables.js", AWAITABLE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .get::<Awaitable<i32>>("directInvalid")
                .await
                .unwrap()
                .await
                .unwrap_err()
                .to_string(),
            module
                .get::<Awaitable<i32>>("promisedInvalid")
                .await
                .unwrap()
                .await
                .unwrap_err()
                .to_string(),
        );
    }
}
