use std::ptr::NonNull;

use guestjs_core::errors::Error;
use rquickjs::{
    AsyncRuntime,
    Persistent,
    Value,
    qjs::JSContext,
};

use crate::{
    context::Token,
    error::Capture,
};

#[derive(Clone)]
pub struct Owned {
    value: Persistent<Value<'static>>,
    context: NonNull<JSContext>,
    runtime: AsyncRuntime,
}

impl Owned {
    pub fn new<'js>(
        token: Token<'js>,
        value: Value<'js>,
    ) -> Self {
        Self {
            value: Persistent::save(&token.ctx(), value),
            context: token.pointer(),
            runtime: token.runtime(),
        }
    }

    pub fn restore<'js>(
        &self,
        token: Token<'js>,
    ) -> Result<Value<'js>, Error> {
        match self.context == token.pointer() {
            true => self
                .value
                .clone()
                .restore(&token.ctx())
                .capture(token),
            false => Err(Error::foreign_context()),
        }
    }

    pub fn is_same(&self, other: &Owned) -> bool {
        self.value == other.value
    }
}
