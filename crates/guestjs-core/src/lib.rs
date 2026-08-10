#[allow(unused_extern_crates)]
extern crate self as guestjs_core;

pub mod errors;
pub mod execution;
pub mod handle;
pub mod host;
pub mod marshal;
pub mod native;
pub mod runtime;
pub mod transpiler;

pub(crate) mod registry;

#[doc(hidden)]
pub mod __private {
    pub use rquickjs::Value as JsValue;
    use serde::{Serialize, de::DeserializeOwned};

    use crate::{errors::Error, runtime::Scope};

    /// Converts a serializable Rust value into a JavaScript value.
    pub fn to_value<'js, T>(value: T, scope: &Scope<'js>) -> Result<JsValue<'js>, Error>
    where
        T: Serialize,
    {
        rquickjs_serde::to_value(scope.ctx().clone(), value)
            .map_err(|error| Error::sourced_conversion(error.to_string(), Some(error)))
    }

    /// Converts a JavaScript value into a deserializable Rust value.
    pub fn from_value<T>(value: JsValue<'_>) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        rquickjs_serde::from_value(value)
            .map_err(|error| Error::sourced_conversion(error.to_string(), Some(error)))
    }
}
