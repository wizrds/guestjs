use guestjs_core::{
    backend::{
        interrupt::{
            BackendInterrupt,
            InterruptCondition,
        },
        Tok,
    },
    errors::Error,
};

use crate::backend::QuickJs;

impl BackendInterrupt for QuickJs {
    fn set_interrupt_condition(
        engine: &Self::Engine,
        condition: InterruptCondition,
    ) {
        engine
            .interrupt()
            .set_condition(condition);
    }

    fn check<'js>(token: Tok<'js, Self>) -> Result<(), Error> {
        match token
            .interrupt()
            .should_interrupt()
        {
            true => Err(Error::interrupted()),
            false => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        rc::Rc,
        task::{
            self,
            Poll,
        },
    };

    use futures::task::noop_waker_ref;
    use guestjs_core::backend::{
        interrupt::BackendInterrupt,
        modules::BackendModules,
        promises::BackendPromises,
        Backend,
    };

    use crate::{
        backend::QuickJs,
        engine::Config,
    };

    #[test]
    fn condition_interrupts_guest_code_while_it_is_met() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();
        let met = Rc::new(Cell::new(true));

        QuickJs::set_interrupt_condition(&engine, {
            let met = met.clone();

            Rc::new(move || met.get())
        });

        QuickJs::enter(&engine, &context, |token| {
            assert!(
                QuickJs::check(token)
                    .is_err_and(|error| error.is_interrupted()),
            );
            assert!(
                QuickJs::eval(
                    token,
                    "try { for (;;) {} } catch { 'caught'; }",
                    "loop.js",
                )
                .is_err_and(|error| error.is_interrupted()),
            );
        });

        met.set(false);

        QuickJs::enter(&engine, &context, |token| {
            assert!(QuickJs::check(token).is_ok());
            assert_eq!(
                QuickJs::eval(token, "1 + 1", "sum.js")
                    .unwrap()
                    .as_number(),
                Some(2.0),
            );
        });
    }

    #[test]
    fn condition_interrupts_jobs() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();
        let mut task = task::Context::from_waker(noop_waker_ref());

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::eval(
                token,
                "queueMicrotask(() => { for (;;) {} });",
                "queue.js",
            )
            .unwrap();
        });

        QuickJs::set_interrupt_condition(&engine, Rc::new(|| true));

        assert!(matches!(
            QuickJs::poll_jobs(&engine, &mut task),
            Poll::Ready(Err(error)) if error.is_interrupted(),
        ));
    }
}
