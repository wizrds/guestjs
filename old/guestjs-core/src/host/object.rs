use rquickjs::{CatchResultExt, Object as JsObject, Value as JsValue};

use crate::{
    errors::Error,
    host::namespace::Namespace,
    marshal::{ToGuest, ToGuestBound},
    runtime::Scope,
};

/// A Rust-defined object exposed to guest code.
pub struct HostObject {
    namespace: Namespace,
}

impl HostObject {
    /// Creates a host object.
    pub fn build<F>(build: F) -> Self
    where
        F: FnOnce(&mut Namespace),
    {
        let mut namespace = Namespace::new();

        build(&mut namespace);

        Self { namespace }
    }
}

impl ToGuest for HostObject {
    fn to_guest<'js>(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        let object = JsObject::new(scope.ctx().clone()).catch(scope.ctx())?;

        self.namespace.apply(scope, &object)?;

        Ok(object.into_value())
    }
}

impl<'js> ToGuestBound<'js> for HostObject {
    fn to_guest_bound(self, scope: &Scope<'js>) -> Result<JsValue<'js>, Error> {
        self.to_guest(scope)
    }
}
