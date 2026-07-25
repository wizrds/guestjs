use std::rc::Rc;

use rquickjs::{
    CatchResultExt, Function as JsFunction, Persistent, Value, function::Args as JsArgs,
};

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestArgs, ToGuestArgsBound, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest function.
#[derive(Clone)]
pub struct Function {
    value: Persistent<JsFunction<'static>>,
    context: Rc<GuestContext>,
}

impl Function {
    pub(crate) fn new(value: Persistent<JsFunction<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context }
    }

    /// Binds the function to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundFunction<'js>, Error> {
        Ok(BoundFunction::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    /// Calls the guest function.
    pub async fn call<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs,
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(
                &scope,
                self.bind(&scope)?
                    .call_value(args.into_args(&scope)?)?,
            )
        })
        .await
    }
}

impl ToGuest for Function {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        Ok(Value::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js> ToGuestBound<'js> for Function {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Function {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        Ok(Function::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_function()
                    .ok_or_else(|| Error::conversion("expected a function"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl FromGuestBound for Function {
    type Bound<'js> = BoundFunction<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: Value<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundFunction::new(
            value
                .into_function()
                .ok_or_else(|| Error::conversion("expected a function"))?,
            scope.clone(),
        ))
    }
}

/// A guest function bound to a scope.
pub struct BoundFunction<'js> {
    value: JsFunction<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundFunction<'js> {
    pub(crate) fn new(value: JsFunction<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
    }

    /// Calls the guest function.
    pub fn call<A, R>(&self, args: A) -> Result<R::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        R: FromGuestBound,
    {
        R::from_guest_bound(&self.scope, self.call_value(args.into_bound_args(&self.scope)?)?)
    }

    /// Converts the function into an owned handle.
    pub fn into_owned(self) -> Result<Function, Error> {
        Ok(Function::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }

    fn call_value(&self, args: JsArgs<'js>) -> Result<Value<'js>, Error> {
        self.value
            .call_arg::<Value>(args)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js> ToGuestBound<'js> for BoundFunction<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        Ok(Value::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use super::Function;
    use crate::{
        errors::Error,
        handle::{Class, Instance, Object, Promise},
        runtime::{Runtime, Scope},
    };

    const FUNCTION_SOURCE: &str = r#"
        export function identity(value) {
            return value;
        }

        export function makeFunction() {
            return (value) => value + 1;
        }

        export function makeObject() {
            return {
                value: 7,
            };
        }

        export class Counter {
            constructor(value) {
                this.value = value;
            }
        }

        export function makeClass() {
            return Counter;
        }

        export function makeInstance() {
            return new Counter(8);
        }

        export async function double(value) {
            return value * 2;
        }
    "#;

    #[tokio::test]
    async fn call_held_function() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        guest
            .eval::<()>("globalThis.add = (a, b) => a + b")
            .await
            .unwrap();

        let add = guest
            .globals()
            .await
            .unwrap()
            .get::<Function>("add")
            .await
            .unwrap();

        assert_eq!(
            add.call::<_, i32>((2, 3))
                .await
                .unwrap(),
            5
        );
        assert_eq!(
            add.call::<_, i32>((10, 20))
                .await
                .unwrap(),
            30
        );
    }

    #[tokio::test]
    async fn bound_calls_propagate_handles_and_inputs() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("functions.js", FUNCTION_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .function("makeFunction")
                .await
                .unwrap()
                .call::<_, Function>(())
                .await
                .unwrap()
                .call::<_, i32>((4,))
                .await
                .unwrap(),
            5,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    let module = module.bind(&scope)?;
                    let function = module
                        .function("makeFunction")?
                        .call::<_, Function>(())?;
                    let object = module
                        .function("makeObject")?
                        .call::<_, Object>(())?;

                    assert_eq!(function.call::<_, i32>((1,))?, 2);
                    assert_eq!(function.call::<_, i32>((5,))?, 6);
                    assert_eq!(object.get::<i32>("value")?, 7);
                    assert_eq!(
                        module
                            .function("identity")?
                            .call::<_, Object>((object,))?
                            .get::<i32>("value")?,
                        7,
                    );
                    assert_eq!(
                        module
                            .function("makeClass")?
                            .call::<_, Class>(())?
                            .construct((5,))?
                            .get::<i32>("value")?,
                        5,
                    );
                    assert_eq!(
                        module
                            .function("makeInstance")?
                            .call::<_, Instance>(())?
                            .get::<i32>("value")?,
                        8,
                    );
                    assert_eq!(
                        module
                            .function("double")?
                            .call::<_, Promise<i32>>((6,))?
                            .await?,
                        12,
                    );

                    module
                        .function("makeFunction")?
                        .call::<_, Function>(())?
                        .into_owned()
                })
                .await
                .unwrap()
                .call::<_, i32>((9,))
                .await
                .unwrap(),
            10,
        );
    }

    #[tokio::test]
    async fn detached_function_cannot_be_promoted() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let function = guest
            .guest_module("detached.js", FUNCTION_SOURCE)
            .await
            .unwrap()
            .function("identity")
            .await
            .unwrap();

        assert_eq!(
            function
                .call::<_, i32>((3,))
                .await
                .unwrap(),
            3,
        );

        guest
            .scope(async move |scope| {
                assert!(matches!(
                    function
                        .bind(&Scope::detached(scope.ctx().clone()))?
                        .into_owned(),
                    Err(Error::Unexpected { message, .. })
                        if message == "cannot build an owned guest handle on detached scope",
                ));

                Ok(())
            })
            .await
            .unwrap();
    }
}
