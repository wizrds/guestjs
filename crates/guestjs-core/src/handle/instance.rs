use std::rc::Rc;

use rquickjs::{
    CatchResultExt, Function as JsFunction, Object as JsObject, Persistent, Value,
    function::Args as JsArgs,
};

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestArgs, ToGuestArgsBound, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest instance.
#[derive(Clone)]
pub struct Instance {
    value: Persistent<JsObject<'static>>,
    context: Rc<GuestContext>,
}

impl Instance {
    pub(crate) fn new(value: Persistent<JsObject<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context }
    }

    /// Binds the instance to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundInstance<'js>, Error> {
        Ok(BoundInstance::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    /// Calls a guest method.
    pub async fn call<A, R>(&self, method: &str, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs,
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(
                &scope,
                self.bind(&scope)?
                    .call_value(method, args.into_args(&scope)?)?,
            )
        })
        .await
    }

    /// Returns a property value.
    pub async fn get<R>(&self, property: &str) -> Result<R::Owned, Error>
    where
        R: FromGuest,
    {
        Scope::with(&self.context, async move |scope| {
            R::from_guest(&scope, self.bind(&scope)?.get_value(property)?)
        })
        .await
    }

    /// Sets a property value.
    pub async fn set<V>(&self, property: &str, value: V) -> Result<(), Error>
    where
        V: ToGuest,
    {
        Scope::with(&self.context, async move |scope| {
            self.bind(&scope)?
                .set_value(property, value.to_guest(&scope)?)
        })
        .await
    }
}

impl ToGuest for Instance {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        Ok(Value::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js> ToGuestBound<'js> for Instance {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Instance {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        Ok(Instance::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_object()
                    .ok_or_else(|| Error::conversion("expected an object"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl FromGuestBound for Instance {
    type Bound<'js> = BoundInstance<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: Value<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundInstance::new(
            value
                .into_object()
                .ok_or_else(|| Error::conversion("expected an object"))?,
            scope.clone(),
        ))
    }
}

/// A guest instance bound to a scope.
pub struct BoundInstance<'js> {
    value: JsObject<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundInstance<'js> {
    pub(crate) fn new(value: JsObject<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
    }

    /// Calls a guest method.
    pub fn call<A, R>(&self, method: &str, args: A) -> Result<R::Bound<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
        R: FromGuestBound,
    {
        R::from_guest_bound(
            &self.scope,
            self.call_value(method, args.into_bound_args(&self.scope)?)?,
        )
    }

    /// Returns a property value.
    pub fn get<R>(&self, property: &str) -> Result<R::Bound<'js>, Error>
    where
        R: FromGuestBound,
    {
        R::from_guest_bound(&self.scope, self.get_value(property)?)
    }

    /// Sets a property value.
    pub fn set<V>(&self, property: &str, value: V) -> Result<(), Error>
    where
        V: ToGuestBound<'js>,
    {
        self.set_value(property, value.to_guest_bound(&self.scope)?)
    }

    /// Converts the instance into an owned handle.
    pub fn into_owned(self) -> Result<Instance, Error> {
        Ok(Instance::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }

    fn call_value(&self, method: &str, mut args: JsArgs<'js>) -> Result<Value<'js>, Error> {
        args.this(self.value.clone())
            .catch(self.scope.ctx())?;

        self.value
            .get::<_, JsFunction>(method)
            .catch(self.scope.ctx())?
            .call_arg(args)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    fn get_value(&self, property: &str) -> Result<Value<'js>, Error> {
        self.value
            .get(property)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    fn set_value(&self, property: &str, value: Value<'js>) -> Result<(), Error> {
        self.value
            .set(property, value)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js> ToGuestBound<'js> for BoundInstance<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        Ok(Value::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::{Function, Object},
        runtime::Runtime,
    };

    const INSTANCE_SOURCE: &str = r#"
        export function makeObject(value) {
            return {
                value,
            };
        }

        export class Holder {
            constructor(value) {
                this.value = value;
            }

            getObject() {
                return this.value;
            }

            getFunction() {
                return (offset) => this.value.value + offset;
            }

            replace(value) {
                this.value = value;
                return this.value;
            }
        }
    "#;

    #[tokio::test]
    async fn construct_and_call_instance() {
        let counter = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "counter.js",
                "export class Counter {\n\
                     constructor(start) { this.n = start; }\n\
                     add(x) { this.n += x; return this.n; }\n\
                 }",
            )
            .await
            .unwrap()
            .class("Counter")
            .await
            .unwrap()
            .construct((10,))
            .await
            .unwrap();

        assert_eq!(
            counter
                .call::<_, i32>("add", (5,))
                .await
                .unwrap(),
            15
        );
        assert_eq!(counter.get::<i32>("n").await.unwrap(), 15);

        counter.set("n", 20).await.unwrap();

        assert_eq!(
            counter
                .call::<_, i32>("add", (5,))
                .await
                .unwrap(),
            25
        );
    }

    #[tokio::test]
    async fn batch_instance_operations() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        let class = guest
            .guest_module(
                "counter.js",
                "export class Counter {\n\
                     constructor(start) { this.n = start; }\n\
                     add(x) { this.n += x; return this.n; }\n\
                 }",
            )
            .await
            .unwrap()
            .class("Counter")
            .await
            .unwrap();

        assert_eq!(
            class
                .construct((2,))
                .await
                .unwrap()
                .get::<i32>("n")
                .await
                .unwrap(),
            2,
        );

        guest
            .scope(async move |scope| {
                let counter = class.bind(&scope)?.construct((10,))?;

                assert_eq!(counter.call::<_, i32>("add", (5,))?, 15);

                counter.set("n", 20)?;

                assert_eq!(counter.get::<i32>("n")?, 20);

                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bound_instance_composes_handle_results_and_inputs() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("instances.js", INSTANCE_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .class("Holder")
                .await
                .unwrap()
                .construct((module
                    .function("makeObject")
                    .await
                    .unwrap()
                    .call::<_, Object>((2,))
                    .await
                    .unwrap(),))
                .await
                .unwrap()
                .call::<_, Object>("getObject", ())
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            2,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    let module = module.bind(&scope)?;
                    let holder = module
                        .class("Holder")?
                        .construct((module
                            .function("makeObject")?
                            .call::<_, Object>((3,))?,))?;

                    assert_eq!(
                        holder
                            .get::<Object>("value")?
                            .get::<i32>("value")?,
                        3,
                    );
                    assert_eq!(
                        holder
                            .call::<_, Object>("getObject", ())?
                            .get::<i32>("value")?,
                        3,
                    );
                    assert_eq!(
                        holder
                            .call::<_, Function>("getFunction", ())?
                            .call::<_, i32>((4,))?,
                        7,
                    );

                    holder.set(
                        "value",
                        module
                            .function("makeObject")?
                            .call::<_, Object>((8,))?,
                    )?;
                    assert_eq!(
                        holder
                            .call::<_, Object>(
                                "replace",
                                (module
                                    .function("makeObject")?
                                    .call::<_, Object>((11,))?,),
                            )?
                            .get::<i32>("value")?,
                        11,
                    );

                    holder.into_owned()
                })
                .await
                .unwrap()
                .get::<Object>("value")
                .await
                .unwrap()
                .get::<i32>("value")
                .await
                .unwrap(),
            11,
        );
    }
}
