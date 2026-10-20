use std::{
    cell::{Cell, RefCell},
    pin::pin,
    rc::Rc,
    task::{self, Poll},
};

use futures::FutureExt;
use guestjs_core::{backend::interrupt::InterruptCondition, errors::Error};
use rquickjs::{
    AsyncRuntime, JsLifetime,
    runtime::{AsyncWeakRuntime, InterruptHandler},
};

use crate::{
    context::{Context, Token},
    error::{Capture, Thrown},
};

#[derive(Clone, Default)]
pub struct Interrupt {
    condition: Rc<RefCell<Option<InterruptCondition>>>,
}

unsafe impl<'js> JsLifetime<'js> for Interrupt {
    type Changed<'to> = Interrupt;
}

impl Interrupt {
    pub fn set_condition(&self, condition: InterruptCondition) {
        *self.condition.borrow_mut() = Some(condition);
    }

    pub fn should_interrupt(&self) -> bool {
        self.condition
            .borrow()
            .as_ref()
            .is_some_and(|condition| condition())
    }

    pub fn handler(&self) -> InterruptHandler {
        let interrupt = self.clone();

        Box::new(move || interrupt.should_interrupt())
    }
}

pub struct DepthGuard<'a> {
    depth: &'a Cell<usize>,
}

impl<'a> DepthGuard<'a> {
    pub fn new(depth: &'a Cell<usize>) -> Self {
        depth.set(depth.get() + 1);

        Self { depth }
    }
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.depth.set(self.depth.get() - 1);
    }
}

pub struct WeakRuntime {
    runtime: AsyncWeakRuntime,
}

unsafe impl<'js> JsLifetime<'js> for WeakRuntime {
    type Changed<'to> = WeakRuntime;
}

impl WeakRuntime {
    pub fn new(runtime: &AsyncRuntime) -> Self {
        Self { runtime: runtime.weak() }
    }

    pub fn upgrade(&self) -> Option<AsyncRuntime> {
        self.runtime.try_ref()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    memory_limit: Option<usize>,
    max_stack_size: Option<usize>,
    gc_threshold: Option<usize>,
}

impl Config {
    pub fn memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = Some(limit);

        self
    }

    pub fn max_stack_size(mut self, limit: usize) -> Self {
        self.max_stack_size = Some(limit);

        self
    }

    pub fn gc_threshold(mut self, threshold: usize) -> Self {
        self.gc_threshold = Some(threshold);

        self
    }

    pub async fn apply(&self, runtime: &AsyncRuntime) {
        if let Some(limit) = self.memory_limit {
            runtime.set_memory_limit(limit).await;
        }

        if let Some(limit) = self.max_stack_size {
            runtime.set_max_stack_size(limit).await;
        }

        if let Some(threshold) = self.gc_threshold {
            runtime
                .set_gc_threshold(threshold)
                .await;
        }
    }
}

pub struct Engine {
    runtime: AsyncRuntime,
    depth: Cell<usize>,
    interrupt: Interrupt,
}

impl Engine {
    pub fn new(config: Config) -> Result<Self, Error> {
        let runtime = AsyncRuntime::new().convert()?;
        let interrupt = Interrupt::default();

        async {
            config.apply(&runtime).await;

            runtime
                .set_interrupt_handler(Some(interrupt.handler()))
                .await;
        }
        .now_or_never()
        .expect("runtime lock is held elsewhere");

        Ok(Self {
            runtime,
            depth: Cell::default(),
            interrupt,
        })
    }

    pub fn runtime(&self) -> &AsyncRuntime {
        &self.runtime
    }

    pub fn interrupt(&self) -> &Interrupt {
        &self.interrupt
    }

    pub fn is_entered(&self) -> bool {
        self.depth.get() > 0
    }

    pub fn enter<F, R>(&self, context: &Context, f: F) -> R
    where
        F: for<'js> FnOnce(Token<'js>) -> R,
    {
        match self.is_entered() {
            true => {
                let _guard = DepthGuard::new(&self.depth);

                context.with_held_lock(f)
            }
            false => context.with(|token| {
                let _guard = DepthGuard::new(&self.depth);

                f(token)
            }),
        }
    }

    pub fn poll_jobs(&self, context: &mut task::Context<'_>) -> Poll<Result<bool, Error>> {
        if self.is_entered() {
            return Poll::Ready(Ok(false));
        }

        let _guard = DepthGuard::new(&self.depth);

        pin!(self.runtime.execute_pending_job())
            .poll_unpin(context)
            .map_err(|exception| {
                exception
                    .0
                    .with(|ctx| Thrown(ctx.catch()).into_error(Token::from_ctx(&ctx)))
                    .now_or_never()
                    .expect("runtime lock is held elsewhere")
            })
    }

    pub fn run_gc(&self) -> Result<(), Error> {
        if self.is_entered() {
            return Err(Error::unsupported("garbage collection requires an unentered engine"));
        }

        let _guard = DepthGuard::new(&self.depth);

        self.runtime
            .run_gc()
            .now_or_never()
            .ok_or_else(|| Error::engine("runtime lock is held elsewhere"))
    }
}
