use std::{
    future::Future,
    rc::Rc,
};

use crate::{
    backend::{
        callables::BackendCallables,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::BackendValues,
        Backend,
        Val,
    },
    driver::host_futures::{
        PendingHostFutures,
        PendingResult,
    },
    enter::Enter,
    errors::Error,
    host::{
        declaration::Declaration,
        state::HostState,
    },
    marshal::{
        args::Args,
        ToGuest,
    },
};

pub struct CallContext<'e, 'js, B: Backend> {
    enter: &'e Enter<'js, B>,
    state: &'e HostState,
}

impl<'e, 'js, B: Backend> CallContext<'e, 'js, B> {
    pub fn new(enter: &'e Enter<'js, B>, state: &'e HostState) -> Self {
        Self {
            enter,
            state,
        }
    }

    pub fn enter(&self) -> &'e Enter<'js, B> {
        self.enter
    }

    pub fn state(&self) -> &'e HostState {
        self.state
    }
}

pub type FunctionBody<B> = Rc<
    dyn for<'e, 'js> Fn(
        &CallContext<'e, 'js, B>,
        &Args<'js, B>,
    ) -> Result<Val<'js, B>, Error>,
>;

pub struct FunctionDeclaration<B: Backend> {
    body: FunctionBody<B>,
}

impl<B: BackendValues> FunctionDeclaration<B> {
    pub fn new<F, R>(function: F) -> Self
    where
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        Self {
            body: Rc::new(move |context, args| {
                function(context, args)?.to_guest(context.enter())
            }),
        }
    }
}

impl<B> FunctionDeclaration<B>
where
    B: BackendPromises + BackendExceptions,
{
    pub fn new_async<F, Fut, R>(function: F) -> Self
    where
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<Fut, Error>
            + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        Self {
            body: Rc::new(move |context, args| {
                PendingHostFutures::of(context.enter().guest()).register(
                    context.enter(),
                    Box::pin(function(context, args).map(|future| {
                        async move {
                            future.await.map::<PendingResult<B>, _>(|value| {
                                Box::new(move |enter| value.to_guest(enter))
                            })
                        }
                    })?),
                )
            }),
        }
    }
}

impl<B: Backend> Clone for FunctionDeclaration<B> {
    fn clone(&self) -> Self {
        Self {
            body: self.body.clone(),
        }
    }
}

impl<B: BackendCallables> Declaration<B> for FunctionDeclaration<B> {
    fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
        name: &str,
    ) -> Result<Val<'js, B>, Error> {
        B::function(
            enter.token(),
            name,
            0,
            enter.guest().raw_body({
                let body = self.body.clone();
                let state = state.clone();

                move |enter, call| {
                    body(&CallContext::new(enter, &state), &Args::new(call))
                }
            }),
        )
    }
}

impl<B: BackendCallables> ToGuest<B> for FunctionDeclaration<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.realise(enter, &HostState::empty(), "")
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::{
        backend::{
            callables::BackendCallables,
            exceptions::BackendExceptions,
            promises::BackendPromises,
            tests::Stub,
        },
        errors::Error,
        guest::Guest,
        host::{
            declaration::Declaration,
            function::{
                CallContext,
                FunctionDeclaration,
            },
            state::HostState,
        },
        marshal::ToGuest,
        runtime::Runtime,
    };

    #[test]
    fn context_carries_its_entry() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();

        guest
            .enter(|enter| {
                assert_eq!(
                    CallContext::new(enter, &HostState::empty())
                        .enter()
                        .guest()
                        .id(),
                    guest.id(),
                );

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn context_carries_its_captured_state() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();
        let state = HostState::new(7_u8);

        guest
            .enter(|enter| {
                assert!(Rc::ptr_eq(
                    &CallContext::new(enter, &state)
                        .state()
                        .get::<u8>()
                        .unwrap(),
                    &state.get::<u8>().unwrap(),
                ));

                Ok(())
            })
            .unwrap();
    }

    #[allow(dead_code)]
    fn capabilities<B>()
    where
        B: BackendCallables + BackendPromises + BackendExceptions,
    {
        fn declaration<B: BackendCallables, T: Declaration<B>>() {}
        fn to<B: BackendCallables, T: ToGuest<B>>() {}
        fn clone<T: Clone>() {}

        let _ = FunctionDeclaration::<B>::new(|_, _| Ok::<_, Error>(1.0));
        let _ = FunctionDeclaration::<B>::new_async(|_, _| {
            Ok::<_, Error>(async { Ok::<_, Error>(1.0) })
        });

        declaration::<B, FunctionDeclaration<B>>();
        to::<B, FunctionDeclaration<B>>();
        clone::<FunctionDeclaration<B>>();
    }
}
