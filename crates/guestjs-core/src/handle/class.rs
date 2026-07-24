use std::sync::Arc;

use rquickjs::{
    CatchResultExt, Constructor as JsConstructor, Object as JsObject, Persistent, Value,
    function::Args as JsArgs,
};

use crate::{
    errors::Error,
    handle::{BoundInstance, Instance},
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestArgs, ToGuestArgsBound, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest class.
#[derive(Clone)]
pub struct Class {
    value: Persistent<JsConstructor<'static>>,
    context: Arc<GuestContext>,
}

impl Class {
    pub(crate) fn new(
        value: Persistent<JsConstructor<'static>>,
        context: Arc<GuestContext>,
    ) -> Self {
        Self { value, context }
    }

    /// Binds the class to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundClass<'js>, Error> {
        Ok(BoundClass::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
    }

    /// Constructs a guest instance.
    pub async fn construct<A>(&self, args: A) -> Result<Instance, Error>
    where
        A: ToGuestArgs,
    {
        Scope::with(&self.context, async move |scope| {
            BoundInstance::new(
                self.bind(&scope)?
                    .construct_value(args.into_args(&scope)?)?,
                scope,
            )
            .into_owned()
        })
        .await
    }
}

impl ToGuest for Class {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        Ok(Value::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js> ToGuestBound<'js> for Class {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Class {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: Value<'js>) -> Result<Self::Owned, Error> {
        Ok(Class::new(
            Persistent::save(
                scope.ctx(),
                value
                    .into_constructor()
                    .ok_or_else(|| Error::conversion("expected a class"))?,
            ),
            scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }
}

impl FromGuestBound for Class {
    type Bound<'js> = BoundClass<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: Value<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundClass::new(
            value
                .into_constructor()
                .ok_or_else(|| Error::conversion("expected a class"))?,
            scope.clone(),
        ))
    }
}

/// A guest class bound to a scope.
pub struct BoundClass<'js> {
    value: JsConstructor<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundClass<'js> {
    pub(crate) fn new(value: JsConstructor<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
    }

    /// Constructs a guest instance.
    pub fn construct<A>(&self, args: A) -> Result<BoundInstance<'js>, Error>
    where
        A: ToGuestArgsBound<'js>,
    {
        Ok(BoundInstance::new(
            self.construct_value(args.into_bound_args(&self.scope)?)?,
            self.scope.clone(),
        ))
    }

    /// Converts the class into an owned handle.
    pub fn into_owned(self) -> Result<Class, Error> {
        Ok(Class::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }

    fn construct_value(&self, args: JsArgs<'js>) -> Result<JsObject<'js>, Error> {
        self.value
            .construct_args(args)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js> ToGuestBound<'js> for BoundClass<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<Value<'js>, Error> {
        Ok(Value::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::Runtime;

    const CLASS_SOURCE: &str = r#"
        export class Counter {
            constructor(value) {
                this.value = value;
            }

            increment() {
                return ++this.value;
            }
        }
    "#;

    #[tokio::test]
    async fn promoted_class_constructs_owned_instances() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("classes.js", CLASS_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .class("Counter")
                .await
                .unwrap()
                .construct((1,))
                .await
                .unwrap()
                .call::<_, i32>("increment", ())
                .await
                .unwrap(),
            2,
        );
        assert_eq!(
            guest
                .scope(async move |scope| {
                    module
                        .bind(&scope)?
                        .class("Counter")?
                        .into_owned()
                })
                .await
                .unwrap()
                .construct((9,))
                .await
                .unwrap()
                .call::<_, i32>("increment", ())
                .await
                .unwrap(),
            10,
        );
    }
}
