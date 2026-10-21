use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    pin::pin,
    rc::Rc,
    task::{self, Poll},
};

use futures::FutureExt;
use guestjs_core::{
    backend::interrupt::InterruptCondition,
    errors::{BorrowKind, Error},
};
use rquickjs::{
    AsyncRuntime, JsLifetime,
    runtime::{AsyncWeakRuntime, InterruptHandler},
};

use crate::{
    context::{Context, Token},
    error::{Capture, Thrown},
};

#[derive(Clone, Copy)]
enum Loan {
    Shared(usize),
    Exclusive,
}

impl Loan {
    fn kind(self) -> BorrowKind {
        match self {
            Loan::Shared(_) => BorrowKind::Shared,
            Loan::Exclusive => BorrowKind::Exclusive,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Storage {
    name: &'static str,
    address: usize,
}

impl Storage {
    pub fn new(name: &'static str, address: usize) -> Self {
        Self { name, address }
    }
}

pub struct LoanGuard {
    loans: Loans,
    storage: Storage,
}

impl LoanGuard {
    fn new(loans: Loans, storage: Storage) -> Self {
        Self { loans, storage }
    }
}

impl Drop for LoanGuard {
    fn drop(&mut self) {
        self.loans.release(self.storage);
    }
}

#[derive(Clone, Default)]
pub struct Loans {
    entries: Rc<RefCell<HashMap<Storage, Loan>>>,
}

unsafe impl<'js> JsLifetime<'js> for Loans {
    type Changed<'to> = Loans;
}

impl Loans {
    pub fn lend(&self, storage: Storage, kind: BorrowKind) -> Result<LoanGuard, Error> {
        let mut entries = self.entries.borrow_mut();

        match (entries.get(&storage).copied(), kind) {
            (None, BorrowKind::Shared) => Ok(Loan::Shared(1)),
            (None, BorrowKind::Exclusive) => Ok(Loan::Exclusive),
            (Some(Loan::Shared(count)), BorrowKind::Shared) => Ok(Loan::Shared(count + 1)),
            (Some(loan), _) => Err(Error::borrow(loan.kind(), storage.name)),
        }
        .map(|loan| {
            entries.insert(storage, loan);

            LoanGuard::new(self.clone(), storage)
        })
    }

    pub fn settled(&self) -> Result<(), Error> {
        match self.entries.borrow().iter().next() {
            None => Ok(()),
            Some((storage, loan)) => Err(Error::borrow(loan.kind(), storage.name)),
        }
    }

    fn release(&self, storage: Storage) {
        let mut entries = self.entries.borrow_mut();

        match entries.get(&storage).copied() {
            Some(Loan::Shared(count)) if count > 1 => {
                entries.insert(storage, Loan::Shared(count - 1));
            }
            _ => {
                entries.remove(&storage);
            }
        }
    }
}

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
    loans: Loans,
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
            loans: Loans::default(),
        })
    }

    pub fn runtime(&self) -> &AsyncRuntime {
        &self.runtime
    }

    pub fn interrupt(&self) -> &Interrupt {
        &self.interrupt
    }

    pub fn loans(&self) -> &Loans {
        &self.loans
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

#[cfg(test)]
mod tests {
    use guestjs_core::errors::{BorrowKind, Error};

    use crate::engine::{Loans, Storage};

    #[test]
    fn loans_share_until_an_exclusive_loan_is_requested() {
        let loans = Loans::default();
        let storage = Storage::new("ArrayBuffer", 1);
        let first = loans
            .lend(storage, BorrowKind::Shared)
            .unwrap();
        let second = loans
            .lend(storage, BorrowKind::Shared)
            .unwrap();

        assert!(matches!(
            loans.lend(storage, BorrowKind::Exclusive),
            Err(Error::Borrow { kind: BorrowKind::Shared, .. }),
        ));

        drop(first);
        drop(second);

        loans
            .lend(storage, BorrowKind::Exclusive)
            .unwrap();
    }

    #[test]
    fn settled_names_the_storage_still_lent() {
        let loans = Loans::default();
        let guard = loans
            .lend(Storage::new("Counter", 1), BorrowKind::Exclusive)
            .unwrap();

        assert_eq!(
            loans.settled().unwrap_err().to_string(),
            Error::borrow(BorrowKind::Exclusive, "Counter").to_string(),
        );

        drop(guard);

        loans.settled().unwrap();
    }

    #[test]
    fn storage_kinds_at_one_address_are_independent() {
        let loans = Loans::default();
        let _buffer = loans
            .lend(Storage::new("ArrayBuffer", 1), BorrowKind::Exclusive)
            .unwrap();
        let _other = loans
            .lend(Storage::new("Counter", 1), BorrowKind::Exclusive)
            .unwrap();
    }
}
