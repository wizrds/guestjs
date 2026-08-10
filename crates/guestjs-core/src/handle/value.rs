use rquickjs::{CatchResultExt, Persistent, Value as JsValue};

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, ToGuest, ToGuestBound},
    runtime::Scope,
};

#[derive(Clone)]
pub struct Value {
    value: Persistent<JsValue<'static>>,
}

impl Value {
    pub(crate) fn new(value: Persistent<JsValue<'static>>) -> Self {
        Self { value }
    }

    pub fn bind<'js, T>(&self, scope: &Scope<'js>) -> Result<T::Bound<'js>, Error>
    where
        T: FromGuestBound,
    {
        T::from_guest_bound(
            scope,
            self.value
                .clone()
                .restore(scope.ctx())
                .catch(scope.ctx())?,
        )
    }
}

impl ToGuest for Value {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.value
            .restore(scope.ctx())
            .catch(scope.ctx())
            .map_err(Into::into)
    }
}

impl<'js> ToGuestBound<'js> for Value {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}

impl FromGuest for Value {
    type Owned = Self;

    fn from_guest<'js>(scope: &Scope<'js>, value: JsValue<'js>) -> Result<Self::Owned, Error> {
        Ok(Self::new(Persistent::save(scope.ctx(), value)))
    }
}

impl FromGuestBound for Value {
    type Bound<'js> = BoundValue<'js>;

    fn from_guest_bound<'js>(
        scope: &Scope<'js>,
        value: JsValue<'js>,
    ) -> Result<Self::Bound<'js>, Error> {
        Ok(BoundValue::new(value, scope.clone()))
    }
}

pub struct BoundValue<'js> {
    value: JsValue<'js>,
    scope: Scope<'js>,
}

impl<'js> BoundValue<'js> {
    pub(crate) fn new(value: JsValue<'js>, scope: Scope<'js>) -> Self {
        Self { value, scope }
    }

    pub fn as_value(&self) -> &JsValue<'js> {
        &self.value
    }

    pub fn into_owned(self) -> Value {
        Value::new(Persistent::save(self.scope.ctx(), self.value))
    }
}

impl<'js> ToGuestBound<'js> for BoundValue<'js> {
    fn to_guest_bound(self, _scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        Ok(self.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        handle::Value,
        host::{Exports, HostModule},
        runtime::Runtime,
    };

    struct ValueHost;

    impl HostModule for ValueHost {
        fn name(&self) -> &str {
            "@host/value"
        }

        fn build(&self, exports: &mut Exports) {
            exports.function("identity", |scope, args| args.get_owned::<Value>(scope, 0));
            exports.function("detached", |scope, args| {
                args.get_owned::<Value>(scope, 0)?;

                Ok(true)
            });
            exports.function("number", |scope, args| {
                args
                    .get_owned::<Value>(scope, 0)?
                    .bind::<i32>(scope)
            });
        }
    }

    async fn value_module() -> crate::handle::Module {
        Runtime::builder()
            .bind(ValueHost)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "value.js",
                "import { detached, identity, number } from \"@host/value\";\n\
                 export function identityValue() {\n\
                     const argument = {};\n\
                     return identity(argument) === argument;\n\
                 }\n\
                 export function detachedValue() { return detached(41); }\n\
                 export function numberValue() { return number(41); }",
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn restores_a_value_within_one_sync_call() {
        let module = value_module().await;
        assert!(
            module
                .function("identityValue")
                .await
                .unwrap()
                .call::<_, bool>(())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn a_value_can_be_carried_from_a_detached_scope() {
        let module = value_module().await;

        assert!(
            module
                .function("detachedValue")
                .await
                .unwrap()
                .call::<_, bool>((41,))
                .await
                .unwrap(),
        );
    }

    #[tokio::test]
    async fn binds_a_value_to_another_type() {
        let module = value_module().await;

        assert_eq!(
            module
                .function("numberValue")
                .await
                .unwrap()
                .call::<_, i32>((41,))
                .await
                .unwrap(),
            41,
        );
    }
}
