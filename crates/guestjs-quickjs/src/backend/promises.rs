use std::task::{self, Poll};

use guestjs_core::{
    backend::{
        Tok, Val,
        promises::{BackendPromises, PromiseState},
    },
    errors::Error,
};
use rquickjs::{Promise, Value};

use crate::{backend::QuickJs, error::Capture};

impl BackendPromises for QuickJs {
    fn new_promise<'js>(
        token: Tok<'js, Self>,
    ) -> Result<(Val<'js, Self>, Val<'js, Self>, Val<'js, Self>), Error> {
        token
            .ctx()
            .promise()
            .map(|(promise, resolve, reject)| {
                (promise.into_value(), resolve.into_value(), reject.into_value())
            })
            .capture(token)
    }

    fn resolve<'js>(token: Tok<'js, Self>, value: Val<'js, Self>) -> Result<Val<'js, Self>, Error> {
        token.loans().settled()?;

        match value.is_promise() {
            true => Ok(value),
            false => token
                .ctx()
                .promise()
                .and_then(|(promise, resolve, _)| {
                    resolve
                        .call::<_, ()>((value,))
                        .map(|_| promise.into_value())
                })
                .capture(token),
        }
    }

    fn state<'js>(
        token: Tok<'js, Self>,
        promise: &Val<'js, Self>,
    ) -> Result<PromiseState<Val<'js, Self>>, Error> {
        match Promise::from_value(promise.clone())
            .capture(token)?
            .result::<Value>()
        {
            None => Ok(PromiseState::Pending),
            Some(Ok(value)) => Ok(PromiseState::Fulfilled(value)),
            Some(Err(_)) => Ok(PromiseState::Rejected(token.ctx().catch())),
        }
    }

    fn poll_jobs(
        engine: &Self::Engine,
        context: &mut task::Context<'_>,
    ) -> Poll<Result<bool, Error>> {
        engine.poll_jobs(context)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        rc::Rc,
        task::{self, Poll},
    };

    use futures::task::noop_waker_ref;
    use guestjs_core::{
        backend::{
            Backend,
            callables::{BackendCallables, RawCall},
            modules::BackendModules,
            promises::{BackendPromises, PromiseState},
            values::BackendValues,
        },
        errors::Error,
    };

    use crate::{backend::QuickJs, engine::Config};

    guestjs_core::fixtures::promises::tests!(QuickJs);

    #[test]
    fn promise_settles_through_its_resolvers() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let (fulfilled, resolve, _) = QuickJs::new_promise(token).unwrap();
            let (rejected, _, reject) = QuickJs::new_promise(token).unwrap();
            let reason = QuickJs::new_object(token).unwrap();

            assert!(matches!(QuickJs::state(token, &fulfilled), Ok(PromiseState::Pending),));

            QuickJs::call(
                token,
                &resolve,
                &QuickJs::new_undefined(token),
                &[QuickJs::new_number(token, 42.0)],
            )
            .unwrap();
            QuickJs::call(token, &reject, &QuickJs::new_undefined(token), &[reason.clone()])
                .unwrap();

            assert!(matches!(
                QuickJs::state(token, &fulfilled),
                Ok(PromiseState::Fulfilled(value)) if value.as_number() == Some(42.0),
            ));
            assert!(matches!(
                QuickJs::state(token, &rejected),
                Ok(PromiseState::Rejected(value)) if QuickJs::same(token, &value, &reason),
            ));
        });
    }

    #[test]
    fn resolve_keeps_promises_and_adopts_thenables() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();
        let mut task = task::Context::from_waker(noop_waker_ref());

        let thenable = QuickJs::enter(&engine, &context, |token| {
            let promise = QuickJs::eval(token, "Promise.resolve(1)", "promise.js").unwrap();

            assert!(QuickJs::same(
                token,
                &QuickJs::resolve(token, promise.clone()).unwrap(),
                &promise,
            ));
            assert!(matches!(
                QuickJs::state(
                    token,
                    &QuickJs::resolve(token, QuickJs::new_number(token, 2.0)).unwrap(),
                ),
                Ok(PromiseState::Fulfilled(value)) if value.as_number() == Some(2.0),
            ));

            let thenable = QuickJs::resolve(
                token,
                QuickJs::eval(token, "({ then(resolve) { resolve(3); } })", "thenable.js").unwrap(),
            )
            .unwrap();

            assert!(matches!(QuickJs::state(token, &thenable), Ok(PromiseState::Pending),));

            QuickJs::detach(token, thenable)
        });

        while let Poll::Ready(Ok(true)) = QuickJs::poll_jobs(&engine, &mut task) {}

        QuickJs::enter(&engine, &context, |token| {
            assert!(matches!(
                QuickJs::state(token, &QuickJs::attach(token, &thenable).unwrap()),
                Ok(PromiseState::Fulfilled(value)) if value.as_number() == Some(3.0),
            ));
            assert!(
                QuickJs::state(token, &QuickJs::new_number(token, 1.0))
                    .is_err_and(|error| error.is_conversion()),
            );
        });
    }

    #[test]
    fn poll_jobs_runs_jobs_only_at_depth_zero() {
        let engine = Rc::new(QuickJs::engine(Config::default()).unwrap());
        let context = Rc::new(QuickJs::new_context(&engine).unwrap());
        let mut task = task::Context::from_waker(noop_waker_ref());

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::call(
                token,
                &QuickJs::eval(token, "queueMicrotask", "queue.js").unwrap(),
                &QuickJs::new_undefined(token),
                &[QuickJs::new_function(
                    token,
                    "probe",
                    0,
                    Rc::new({
                        let engine = engine.clone();
                        let context = context.clone();

                        move |call: RawCall<'_, QuickJs>| {
                            QuickJs::enter(&engine, &context, |token| {
                                QuickJs::eval(token, "globalThis.ran = true;", "probe.js")
                                    .map(|_| ())
                            })?;

                            Ok(QuickJs::new_undefined(call.token))
                        }
                    }),
                )
                .unwrap()],
            )
            .unwrap();

            assert!(matches!(QuickJs::poll_jobs(&engine, &mut task), Poll::Ready(Ok(false)),));
        });

        assert!(matches!(QuickJs::poll_jobs(&engine, &mut task), Poll::Ready(Ok(true)),));
        assert!(matches!(QuickJs::poll_jobs(&engine, &mut task), Poll::Ready(Ok(false)),));

        QuickJs::enter(&engine, &context, |token| {
            assert_eq!(
                QuickJs::eval(token, "ran", "ran.js")
                    .unwrap()
                    .as_bool(),
                Some(true),
            );
        });
    }

    #[test]
    fn throwing_job_is_captured() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();
        let mut task = task::Context::from_waker(noop_waker_ref());

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::eval(
                token,
                r#"queueMicrotask(() => { throw new RangeError("job"); });"#,
                "queue.js",
            )
            .unwrap();
        });

        assert!(matches!(
            QuickJs::poll_jobs(&engine, &mut task),
            Poll::Ready(Err(Error::Guest(exception))) if exception.name() == "RangeError",
        ));
    }
}
