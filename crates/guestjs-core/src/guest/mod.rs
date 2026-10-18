pub mod activity;
pub mod registry;

use std::{
    cell::Cell,
    num::NonZeroU64,
    rc::{
        Rc,
        Weak,
    },
};

use crate::{
    backend::{
        callables::{
            RawBody,
            RawCall,
        },
        interrupt::BackendInterrupt,
        Backend,
        Tok,
        Val,
    },
    enter::Enter,
    errors::Error,
    extensions::Extensions,
    guest::activity::GuestActivity,
    policy::ExecutionPolicy,
    runtime::Runtime,
};

struct GuestState<B: Backend> {
    id: GuestId,
    context_id: B::ContextId,
    closed: Cell<bool>,
    activity: GuestActivity,
    extensions: Extensions,
    context: B::Context,
    runtime: Runtime<B>,
}

impl<B: Backend> GuestState<B> {
    pub fn new(
        runtime: &Runtime<B>,
        policy: ExecutionPolicy,
    ) -> Result<Self, Error> {
        let context = B::new_context(runtime.engine())?;

        Ok(Self {
            id: runtime.next_id(),
            context_id: B::enter(
                runtime.engine(),
                &context,
                B::context_id,
            ),
            closed: Cell::default(),
            activity: GuestActivity::new(policy),
            extensions: Extensions::default(),
            context,
            runtime: runtime.clone(),
        })
    }

    pub fn id(&self) -> GuestId {
        self.id
    }

    pub fn runtime(&self) -> &Runtime<B> {
        &self.runtime
    }

    pub fn context(&self) -> &B::Context {
        &self.context
    }

    pub fn context_id(&self) -> B::ContextId {
        self.context_id
    }

    pub fn activity(&self) -> &GuestActivity {
        &self.activity
    }

    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }

    pub fn close(&self) {
        self.closed.set(true);
    }

    pub fn activate<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce() -> Result<R, Error>,
    {
        let _activation = self.activity.begin()?;

        match f() {
            Err(Error::Interrupted) => Err(self
                .activity
                .due_error()
                .unwrap_or(Error::interrupted())),
            result => result,
        }
    }

    pub fn enter<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'js> FnOnce(Tok<'js, B>) -> Result<R, Error>,
    {
        match self.is_closed() {
            true => Err(Error::closed()),
            false => self.activate(|| {
                B::enter(
                    self.runtime.engine(),
                    &self.context,
                    f,
                )
            }),
        }
    }

    pub fn reenter<'js, F, R>(
        &self,
        token: Tok<'js, B>,
        f: F,
    ) -> Result<R, Error>
    where
        F: FnOnce(Tok<'js, B>) -> Result<R, Error>,
    {
        match (
            self.is_closed(),
            B::context_id(token) == self.context_id,
        ) {
            (true, _) => Err(Error::closed()),
            (false, false) => Err(Error::foreign_context()),
            (false, true) => self.activate(|| f(token)),
        }
    }
}

impl<B: Backend> Drop for GuestState<B> {
    fn drop(&mut self) {
        self.runtime.unregister(self.context_id);
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GuestId(NonZeroU64);

impl GuestId {
    pub fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

pub struct Guest<B: Backend> {
    state: Rc<GuestState<B>>,
}

impl<B: Backend> Clone for Guest<B> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<B: Backend> Guest<B> {
    pub fn new(runtime: &Runtime<B>) -> Result<Self, Error> {
        Self::from_policy(runtime, ExecutionPolicy::default())
    }

    pub fn with_policy(
        runtime: &Runtime<B>,
        policy: ExecutionPolicy,
    ) -> Result<Self, Error>
    where
        B: BackendInterrupt,
    {
        runtime.install_policy_interrupt();

        Self::from_policy(runtime, policy)
    }

    fn from_policy(
        runtime: &Runtime<B>,
        policy: ExecutionPolicy,
    ) -> Result<Self, Error> {
        let guest = Self::from_state(
            Rc::new(GuestState::new(runtime, policy)?),
        );

        runtime.register(&guest);

        Ok(guest)
    }

    fn from_state(state: Rc<GuestState<B>>) -> Self {
        Self {
            state,
        }
    }

    pub fn id(&self) -> GuestId {
        self.state.id()
    }

    pub fn runtime(&self) -> &Runtime<B> {
        self.state.runtime()
    }

    pub fn context(&self) -> &B::Context {
        self.state.context()
    }

    pub fn context_id(&self) -> B::ContextId {
        self.state.context_id()
    }

    pub fn activity(&self) -> &GuestActivity {
        self.state.activity()
    }

    pub fn extensions(&self) -> &Extensions {
        self.state.extensions()
    }

    pub fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    pub fn close(&self) {
        self.state.close();
    }

    pub fn downgrade(&self) -> WeakGuest<B> {
        WeakGuest::from_state(Rc::downgrade(&self.state))
    }

    pub fn activate<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce() -> Result<R, Error>,
    {
        self.state.activate(f)
    }

    pub fn enter<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'js> FnOnce(&Enter<'js, B>) -> Result<R, Error>,
    {
        self.state.enter(|token| f(&Enter::new(token, self.clone())))
    }

    pub fn reenter<'js, F, R>(
        &self,
        token: Tok<'js, B>,
        f: F,
    ) -> Result<R, Error>
    where
        F: FnOnce(&Enter<'js, B>) -> Result<R, Error>,
    {
        self.state.reenter(
            token,
            |token| f(&Enter::new(token, self.clone())),
        )
    }

    pub fn raw_body<F>(&self, body: F) -> RawBody<B>
    where
        F: for<'js> Fn(
                &Enter<'js, B>,
                RawCall<'js, B>,
            ) -> Result<Val<'js, B>, Error>
            + 'static,
    {
        let guest = self.downgrade();

        Rc::new(move |call: RawCall<'_, B>| {
            guest
                .upgrade()
                .ok_or(Error::closed())?
                .reenter(call.token, |enter| body(enter, call))
        })
    }
}

pub struct WeakGuest<B: Backend> {
    state: Weak<GuestState<B>>,
}

impl<B: Backend> Clone for WeakGuest<B> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<B: Backend> WeakGuest<B> {
    fn from_state(state: Weak<GuestState<B>>) -> Self {
        Self {
            state,
        }
    }

    pub fn upgrade(&self) -> Option<Guest<B>> {
        self.state
            .upgrade()
            .map(Guest::from_state)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{
            callables::RawCall,
            tests::Stub,
            Backend,
        },
        errors::Error,
        guest::Guest,
        policy::{
            Cancellation,
            ExecutionPolicy,
        },
        runtime::Runtime,
    };

    #[test]
    fn enter_pairs_the_token_with_its_guest() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();

        assert!(guest
            .enter(|enter| {
                Ok(enter.guest().id() == guest.id()
                    && Stub::context_id(enter.token()) == guest.context_id())
            })
            .unwrap());
    }

    #[test]
    fn closed_guest_refuses_entry() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();

        guest.close();
        guest.close();

        assert!(guest.is_closed());
        assert!(guest.enter(|_| Ok(())).is_err_and(|error| error.is_closed()));
        assert!(
            Stub::enter(
                runtime.engine(),
                guest.context(),
                |token| guest.reenter(token, |_| Ok(())),
            )
            .is_err_and(|error| error.is_closed()),
        );
    }

    #[test]
    fn reenter_rejects_a_foreign_token() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let first = Guest::new(&runtime).unwrap();
        let second = Guest::new(&runtime).unwrap();

        assert!(
            first
                .enter(|enter| second.reenter(enter.token(), |_| Ok(())))
                .is_err_and(|error| error.is_foreign_context()),
        );
    }

    #[test]
    fn raw_body_rebuilds_the_guest_entry() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();
        let context_id = guest.context_id();
        let body = guest.raw_body({
            let id = guest.id();

            move |enter, _| {
                assert_eq!(enter.guest().id(), id);

                Ok(())
            }
        });
        let invoke = || {
            Stub::enter(
                runtime.engine(),
                &context_id,
                |token| {
                    body(RawCall {
                        token,
                        this: (),
                        args: Vec::new(),
                        new_target: None,
                    })
                },
            )
        };

        assert!(invoke().is_ok());

        guest.close();

        assert!(invoke().is_err_and(|error| error.is_closed()));

        drop(guest);

        assert!(invoke().is_err_and(|error| error.is_closed()));
    }

    #[test]
    fn interruption_is_named_by_the_guest_whose_policy_is_due() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let cancellation = Cancellation::default();
        let outer = Guest::with_policy(
            &runtime,
            ExecutionPolicy {
                timeout: None,
                cancellation: Some(cancellation.clone()),
            },
        )
        .unwrap();

        assert!(
            outer.enter(|_| {
                cancellation.cancel();

                assert!(
                    Guest::new(&runtime)
                        .unwrap()
                        .enter(|_| Err::<(), _>(Error::interrupted()))
                        .is_err_and(|error| error.is_interrupted()),
                );

                Err::<(), _>(Error::interrupted())
            })
            .is_err_and(|error| error.is_cancelled()),
        );
    }

    #[test]
    fn due_guest_refuses_entries_and_callbacks() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let cancellation = Cancellation::default();
        let guest = Guest::with_policy(
            &runtime,
            ExecutionPolicy {
                timeout: None,
                cancellation: Some(cancellation.clone()),
            },
        )
        .unwrap();

        cancellation.cancel();

        assert!(
            guest
                .enter(|_| Ok(()))
                .is_err_and(|error| error.is_cancelled()),
        );
        assert!(
            Stub::enter(
                runtime.engine(),
                guest.context(),
                |token| {
                    guest.raw_body(|_, _| Ok(()))(RawCall {
                        token,
                        this: (),
                        args: Vec::new(),
                        new_target: None,
                    })
                },
            )
            .is_err_and(|error| error.is_cancelled()),
        );

        guest.close();

        assert!(guest.enter(|_| Ok(())).is_err_and(|error| error.is_closed()));
    }
}
