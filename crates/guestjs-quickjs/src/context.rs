use std::ptr::NonNull;

use futures::FutureExt;
use guestjs_core::errors::Error;
use rquickjs::{
    AsyncContext,
    AsyncRuntime,
    Ctx,
    markers::Invariant,
    qjs::JSContext,
};

use crate::{
    engine::{
        Engine,
        Interrupt,
        WeakRuntime,
    },
    error::Capture,
};

#[derive(Clone, Copy)]
pub struct Token<'js> {
    pointer: NonNull<JSContext>,
    brand: Invariant<'js>,
}

impl<'js> Token<'js> {
    pub(crate) fn new(pointer: NonNull<JSContext>) -> Self {
        Self {
            pointer,
            brand: Invariant::new(),
        }
    }

    pub fn from_ctx(ctx: &Ctx<'js>) -> Self {
        Self {
            pointer: ctx.as_raw(),
            brand: Invariant::new(),
        }
    }

    pub fn ctx(&self) -> Ctx<'js> {
        unsafe { Ctx::from_raw_invariant(self.pointer, self.brand) }
    }

    pub fn pointer(&self) -> NonNull<JSContext> {
        self.pointer
    }

    pub fn runtime(&self) -> AsyncRuntime {
        self.ctx()
            .userdata::<WeakRuntime>()
            .and_then(|runtime| runtime.upgrade())
            .expect("token outlived its runtime")
    }

    pub fn interrupt(&self) -> Interrupt {
        self.ctx()
            .userdata::<Interrupt>()
            .map(|interrupt| interrupt.clone())
            .expect("context has no interrupt")
    }
}

pub struct Context {
    context: AsyncContext,
    pointer: NonNull<JSContext>,
}

impl Context {
    pub fn new(engine: &Engine) -> Result<Self, Error> {
        if engine.is_entered() {
            return Err(Error::unsupported(
                "a context cannot be created inside an entry",
            ));
        }

        async {
            let context = AsyncContext::full(engine.runtime())
                .await
                .convert()?;

            Ok::<_, Error>(Self {
                pointer: context
                    .with(|ctx| {
                        ctx.store_userdata(WeakRuntime::new(engine.runtime()))
                            .map_err(|_| ())
                            .and_then(|_| {
                                ctx.store_userdata(engine.interrupt().clone())
                                    .map_err(|_| ())
                            })
                            .map(|_| ctx.as_raw())
                    })
                    .await
                    .map_err(|_| {
                        Error::engine("runtime userdata is borrowed")
                    })?,
                context,
            })
        }
        .now_or_never()
        .expect("runtime lock is held elsewhere")
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: for<'js> FnOnce(Token<'js>) -> R,
    {
        self.context
            .with(|ctx| f(Token::from_ctx(&ctx)))
            .now_or_never()
            .expect("runtime lock is held elsewhere")
    }

    pub(crate) fn with_held_lock<F, R>(&self, f: F) -> R
    where
        F: for<'js> FnOnce(Token<'js>) -> R,
    {
        f(Token::new(self.pointer))
    }
}
