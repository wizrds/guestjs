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

/// An owned guest promise awaited outside a scope.
pub struct Promise<T> {
    value: Persistent<JsPromise<'static>>,
    context: Rc<GuestContext>,
    _result: PhantomData<fn() -> T>,
}

impl<T> Promise<T> {
    pub(crate) fn new(value: Persistent<JsPromise<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context, _result: PhantomData }
    }

    /// Binds the promise to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundPromise<'js, T>, Error> {
        Ok(BoundPromise::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }
}

impl<T: 'static> FromGuest for Promise<T> {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Self::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_promise()
                    .ok_or_else(|| Error::conversion("expected a promise"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl<T> FromGuestBound for Promise<T> {
    type Bound<'js> = BoundPromise<'js, T>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundPromise::new(
            value
                .into_promise()
                .ok_or_else(|| Error::conversion("expected a promise"))?,
            scope.clone(),
        ))
    }
}

impl<T> ToGuest for Promise<T> {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js, T> ToGuestBound<'js> for Promise<T> {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl<T> IntoFuture for Promise<T>
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

/// A guest promise awaited within its scope.
pub struct BoundPromise<'js, T> {
    value: JsPromise<'js>,
    scope: Scope<'js>,
    _result: PhantomData<fn() -> T>,
}

impl<'js, T> BoundPromise<'js, T> {
    pub(crate) fn new(value: JsPromise<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope, _result: PhantomData }
    }

    /// Converts the promise into an owned handle.
    pub fn into_owned(self) -> Result<Promise<T>, Error> {
        Ok(Promise::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }

    async fn resolve_value(&self) -> Result<JsValue<'js>, Error> {
        self.value
            .clone()
            .into_future()
            .await
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js, T> IntoFuture for BoundPromise<'js, T>
where
    T: FromGuestBound + 'js,
{
    type Output = Result<T::Bound<'js>, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + 'js>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { T::from_guest_bound(&self.scope, self.resolve_value().await?) })
    }
}

impl<'js, T> ToGuestBound<'js> for BoundPromise<'js, T> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::{Class, Function, Object, Promise},
        marshal::Nullish,
        runtime::Runtime,
    };

    const PROMISE_SOURCE: &str = r#"
        export async function functions() {
            return [
                (value) => value + 1,
                (value) => value * 2,
            ];
        }

        export async function optionalObject(present) {
            return present
                ? { value: 7 }
                : null;
        }

        export async function nullableClass(kind) {
            class Counter {
                constructor(value) {
                    this.value = value;
                }
            }

            if (kind === "undefined") {
                return undefined;
            }

            if (kind === "null") {
                return null;
            }

            return Counter;
        }
    "#;

    #[tokio::test]
    async fn await_guest_promise() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        guest
            .eval::<()>("globalThis.slow = (x) => Promise.resolve(x * 2)")
            .await
            .unwrap();

        assert_eq!(
            guest
                .globals()
                .await
                .unwrap()
                .get::<Function>("slow")
                .await
                .unwrap()
                .call::<_, Promise<i32>>((21,))
                .await
                .unwrap()
                .await
                .unwrap(),
            42,
        );
    }

    #[tokio::test]
    async fn owned_promises_materialize_recursive_handles() {
        let module = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module("owned-promises.js", PROMISE_SOURCE)
            .await
            .unwrap();
        let functions = module
            .function("functions")
            .await
            .unwrap()
            .call::<_, Promise<Vec<Function>>>(())
            .await
            .unwrap()
            .await
            .unwrap();

        assert_eq!(
            functions[0]
                .call::<_, i32>((4,))
                .await
                .unwrap(),
            5
        );
        assert_eq!(
            functions[1]
                .call::<_, i32>((4,))
                .await
                .unwrap(),
            8
        );
        assert_eq!(
            module
                .function("optionalObject")
                .await
                .unwrap()
                .call::<_, Promise<Option<Object>>>((true,))
                .await
                .unwrap()
                .await
                .unwrap()
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            7,
        );

        match module
            .function("nullableClass")
            .await
            .unwrap()
            .call::<_, Promise<Nullish<Class>>>(("class",))
            .await
            .unwrap()
            .await
            .unwrap()
        {
            Nullish::Some(class) => {
                assert_eq!(
                    class
                        .construct((9,))
                        .await
                        .unwrap()
                        .get::<i32>("value")
                        .await
                        .unwrap(),
                    9,
                );
            }
            _ => panic!("expected a class"),
        }
    }

    #[tokio::test]
    async fn bound_promises_materialize_and_promote_recursive_handles() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("bound-promises.js", PROMISE_SOURCE)
            .await
            .unwrap();

        assert!(
            module
                .function("optionalObject")
                .await
                .unwrap()
                .call::<_, Promise<Option<Object>>>((false,))
                .await
                .unwrap()
                .await
                .unwrap()
                .is_none(),
        );

        assert_eq!(
            guest
                .scope(async move |scope| {
                    let module = module.bind(&scope)?;
                    let functions = module
                        .function("functions")?
                        .call::<_, Promise<Vec<Function>>>(())?
                        .await?;

                    assert_eq!(functions[0].call::<_, i32>((2,))?, 3);
                    assert_eq!(functions[1].call::<_, i32>((2,))?, 4);
                    assert_eq!(
                        module
                            .function("optionalObject")?
                            .call::<_, Promise<Option<Object>>>((true,))?
                            .await?
                            .unwrap()
                            .get::<i32>("value")?,
                        7,
                    );

                    match module
                        .function("nullableClass")?
                        .call::<_, Promise<Nullish<Class>>>(("class",))?
                        .await?
                    {
                        Nullish::Some(class) => {
                            assert_eq!(
                                class
                                    .construct((6,))?
                                    .get::<i32>("value")?,
                                6,
                            );
                        }
                        _ => panic!("expected a class"),
                    }

                    module
                        .function("functions")?
                        .call::<_, Promise<Vec<Function>>>(())?
                        .into_owned()
                })
                .await
                .unwrap()
                .await
                .unwrap()
                .into_iter()
                .next()
                .unwrap()
                .call::<_, i32>((10,))
                .await
                .unwrap(),
            11,
        );
    }
}
