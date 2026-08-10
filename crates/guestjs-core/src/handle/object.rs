use std::rc::Rc;

use rquickjs::{CatchResultExt, Object as JsObject, Persistent, Value as JsValue};

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::{GuestContext, Scope},
};

/// An owned guest object.
#[derive(Clone)]
pub struct Object {
    value: Persistent<JsObject<'static>>,
    context: Rc<GuestContext>,
}

impl Object {
    pub(crate) fn new(value: Persistent<JsObject<'static>>, context: Rc<GuestContext>) -> Self {
        Self { value, context }
    }

    /// Binds the object to a scope.
    pub fn bind<'js>(&self, scope: &Scope<'js>) -> Result<BoundObject<'js>, Error> {
        Ok(BoundObject::new(
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
            scope.clone(),
        ))
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

impl ToGuest for Object {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(
            self.value
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        ))
    }
}

impl<'js> ToGuestBound<'js> for Object {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Object {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Object::new(
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

impl FromGuestBound for Object {
    type Bound<'js> = BoundObject<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundObject::new(
            value
                .into_object()
                .ok_or_else(|| Error::conversion("expected an object"))?,
            scope.clone(),
        ))
    }
}

/// A guest object bound to a scope.
pub struct BoundObject<'js> {
    value: JsObject<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundObject<'js> {
    pub(crate) fn new(value: JsObject<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
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

    /// Converts the object into an owned handle.
    pub fn into_owned(self) -> Result<Object, Error> {
        Ok(Object::new(
            Persistent::save(self.scope.ctx(), self.value),
            self.scope
                .parent()
                .ok_or_else(Error::detached_scope)?
                .clone(),
        ))
    }

    fn get_value(&self, property: &str) -> Result<JsValue<'js>, Error> {
        self.value
            .get(property)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }

    fn set_value(&self, property: &str, value: JsValue<'js>) -> Result<(), Error> {
        self.value
            .set(property, value)
            .catch(self.scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js> ToGuestBound<'js> for BoundObject<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(JsValue::from(self.value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{handle::Function, runtime::Runtime};

    const OBJECT_SOURCE: &str = r#"
        export const holder = {
            value: 2,
        };

        export function makeFunction() {
            return (value) => value * 3;
        }
    "#;

    #[tokio::test]
    async fn bound_object_composes_with_function_handles() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();
        let module = guest
            .guest_module("objects.js", OBJECT_SOURCE)
            .await
            .unwrap();

        assert_eq!(
            module
                .object("holder")
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
                    let holder = module.object("holder")?;

                    holder.set(
                        "callback",
                        module
                            .function("makeFunction")?
                            .call::<_, Function>(())?,
                    )?;

                    assert_eq!(
                        holder
                            .get::<Function>("callback")?
                            .call::<_, i32>((4,))?,
                        12,
                    );

                    module.object("holder")?.into_owned()
                })
                .await
                .unwrap()
                .get::<Function>("callback")
                .await
                .unwrap()
                .call::<_, i32>((5,))
                .await
                .unwrap(),
            15,
        );
    }
}
