use crate::{
    backend::{
        exceptions::BackendExceptions, interrupt::BackendInterrupt, modules::BackendModules,
        promises::BackendPromises,
    },
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    errors::Error,
    eval::{GuestEvalExt, GuestGlobalsExt},
    fixtures::guest_fixture,
    handle::{
        iter::AsyncIter,
        object::Object,
        promise::{Awaitable, Promise},
        protocol::{CallableProtocol, ObjectProtocol},
    },
    runtime::Runtime,
};

const COUNTER_SOURCE: &str = r#"
class Counter {
    constructor() {
        this.value = 0;
    }

    [Symbol.asyncIterator]() {
        return this;
    }

    async next() {
        this.value += 1;

        return this.value > 2
            ? { done: true }
            : { value: this.value, done: false };
    }
}
"#;

struct IteratorResults;

impl IteratorResults {
    async fn assert_yield<B>(result: Promise<B, Object<B>>, expected: i64)
    where
        B: BackendPromises + BackendExceptions,
    {
        let result = result.await.unwrap();

        assert_eq!(result.get::<i64, _>("value").unwrap(), expected);
        assert!(!result.get::<bool, _>("done").unwrap());
    }
}

guest_fixture! {
    pub async fn awaits_a_guest_promise<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        async function double(value) {
            return value * 2;
        }
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .globals()
                .unwrap()
                .function("double")
                .unwrap()
                .call::<_, Promise<B, f64>>((21.0,))
                .unwrap()
                .await
                .unwrap(),
            42.0,
        );
    }
}

guest_fixture! {
    pub async fn next_advances_a_plain_async_iterator<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest.exec(COUNTER_SOURCE).unwrap();

        let mut iterator = guest.eval::<AsyncIter<B, i64>>("new Counter()").unwrap();

        assert_eq!(iterator.next().await.unwrap(), Some(1));
        assert_eq!(iterator.next().await.unwrap(), Some(2));
        assert_eq!(iterator.next().await.unwrap(), None);
    }
}

guest_fixture! {
    pub async fn accepts_and_delegates_async_iterables<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest.exec(COUNTER_SOURCE).unwrap();

        for mut iterator in [
            guest.eval::<AsyncIter<B, i64>>("new Counter()").unwrap(),
            guest
                .eval::<Object<B>>("new Counter()")
                .unwrap()
                .cast::<AsyncIter<B, i64>>()
                .unwrap(),
        ] {
            assert_eq!(iterator.next().await.unwrap(), Some(1));
            assert_eq!(iterator.next().await.unwrap(), Some(2));
            assert_eq!(iterator.next().await.unwrap(), None);
        }

        guest
            .exec(
                r#"
        globalThis.non_callable = { [Symbol.asyncIterator]: 42 };
        globalThis.broken = {
            [Symbol.asyncIterator]() {
                throw new RangeError("broken aiter");
            },
        };
        "#,
            )
            .unwrap();

        for source in ["({})", "non_callable"] {
            assert!(guest
                .eval::<AsyncIter<B, i64>>(source)
                .is_err_and(|error| error.is_conversion()));
        }

        assert!(matches!(
            guest.eval::<AsyncIter<B, i64>>("broken"),
            Err(Error::Guest(exception))
                if exception.matches("RangeError")
                    && exception.message() == "broken aiter",
        ));
    }
}

guest_fixture! {
    pub async fn controls_an_async_generator<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        globalThis.generator_closed = false;

        async function* values() {
            try {
                const value = yield 1;
                yield value;
            } catch (error) {
                if (!(error instanceof RangeError)) {
                    throw error;
                }

                yield 3;
            } finally {
                generator_closed = true;
            }
        }
        "#,
            )
            .unwrap();

        let generator = guest.eval::<Object<B>>("values()").unwrap();

        IteratorResults::assert_yield(
            generator
                .call_method::<_, _, Promise<B, Object<B>>>("next", ())
                .unwrap(),
            1,
        )
        .await;
        IteratorResults::assert_yield(
            generator
                .call_method::<_, _, Promise<B, Object<B>>>("next", (2i64,))
                .unwrap(),
            2,
        )
        .await;
        IteratorResults::assert_yield(
            generator
                .call_method::<_, _, Promise<B, Object<B>>>(
                    "throw",
                    (guest.eval::<Object<B>>("new RangeError('boom')").unwrap(),),
                )
                .unwrap(),
            3,
        )
        .await;

        assert!(generator
            .call_method::<_, _, Promise<B, Object<B>>>("return", ())
            .unwrap()
            .await
            .unwrap()
            .get::<bool, _>("done")
            .unwrap());
        assert!(guest.eval::<bool>("generator_closed").unwrap());
    }
}

guest_fixture! {
    pub async fn async_iteration_adopts_thenable_next_results<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        class Numbers {
            constructor() {
                this.remaining = [1, 2, 3];
            }

            [Symbol.asyncIterator]() {
                return this;
            }

            next() {
                const result = this.remaining.length === 0
                    ? { done: true }
                    : { value: this.remaining.shift(), done: false };

                return {
                    then(resolve) {
                        resolve(result);
                    },
                };
            }
        }

        async function collect_numbers() {
            const result = [];

            for await (const value of new Numbers()) {
                result.push(value);
            }

            return result;
        }
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .eval::<Promise<B, Vec<i64>>>("collect_numbers()")
                .unwrap()
                .await
                .unwrap(),
            vec![1, 2, 3],
        );

        let mut iterator = guest.eval::<AsyncIter<B, i64>>("new Numbers()").unwrap();

        assert_eq!(iterator.next().await.unwrap(), Some(1));
        assert_eq!(iterator.next().await.unwrap(), Some(2));
        assert_eq!(iterator.next().await.unwrap(), Some(3));
        assert_eq!(iterator.next().await.unwrap(), None);
    }
}

guest_fixture! {
    pub async fn awaitable_wraps_plain_values<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert_eq!(
            guest
                .eval::<Awaitable<i32>>("7")
                .unwrap()
                .await
                .unwrap(),
            7,
        );
    }
}

guest_fixture! {
    pub async fn awaitable_adopts_thenables<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert_eq!(
            guest
                .eval::<Awaitable<i32>>("({ then(resolve) { resolve(9); } })")
                .unwrap()
                .await
                .unwrap(),
            9,
        );
    }
}

guest_fixture! {
    pub fn strict_promises_still_refuse_plain_values<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert!(guest
            .eval::<Promise<B, i32>>("7")
            .is_err_and(|error| error.is_conversion()));
    }
}

#[macro_export]
macro_rules! __guestjs_backend_promises_tests {
    ($backend:ty) => {
        #[tokio::test]
        async fn awaits_a_guest_promise() {
            $crate::fixtures::promises::awaits_a_guest_promise::<$backend>().await;
        }

        #[tokio::test]
        async fn next_advances_a_plain_async_iterator() {
            $crate::fixtures::promises::next_advances_a_plain_async_iterator::<$backend>().await;
        }

        #[tokio::test]
        async fn accepts_and_delegates_async_iterables() {
            $crate::fixtures::promises::accepts_and_delegates_async_iterables::<$backend>().await;
        }

        #[tokio::test]
        async fn controls_an_async_generator() {
            $crate::fixtures::promises::controls_an_async_generator::<$backend>().await;
        }

        #[tokio::test]
        async fn async_iteration_adopts_thenable_next_results() {
            $crate::fixtures::promises::async_iteration_adopts_thenable_next_results::<$backend>()
                .await;
        }

        #[tokio::test]
        async fn awaitable_wraps_plain_values() {
            $crate::fixtures::promises::awaitable_wraps_plain_values::<$backend>().await;
        }

        #[tokio::test]
        async fn awaitable_adopts_thenables() {
            $crate::fixtures::promises::awaitable_adopts_thenables::<$backend>().await;
        }

        #[test]
        fn strict_promises_still_refuse_plain_values() {
            $crate::fixtures::promises::strict_promises_still_refuse_plain_values::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_promises_tests as tests;
