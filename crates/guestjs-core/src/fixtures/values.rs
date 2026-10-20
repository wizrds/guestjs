use crate::{
    backend::{interrupt::BackendInterrupt, modules::BackendModules},
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    errors::Error,
    eval::GuestEvalExt,
    fixtures::guest_fixture,
    guest::Guest,
    handle::iter::Iter,
    marshal::{FromGuest, collections::Entries},
    runtime::Runtime,
};

struct Mappings;

impl Mappings {
    fn read<B>(guest: &Guest<B>, source: &str) -> Result<Vec<(String, i64)>, Error>
    where
        B: BackendModules,
    {
        guest.enter(|enter| {
            Entries::read(enter, &B::eval(enter.token(), source, "mapping.js")?)?
                .into_iter()
                .map(|(key, value)| {
                    Ok((String::from_guest(enter, key)?, i64::from_guest(enter, value)?))
                })
                .collect()
        })
    }
}

guest_fixture! {
    pub fn vec_reads_guest_arrays<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert_eq!(
            guest.eval::<Vec<i64>>("[1, 2, 3]").unwrap(),
            vec![1, 2, 3],
        );
        assert_eq!(
            guest.eval::<Vec<i64>>("[]").unwrap(),
            Vec::<i64>::new(),
        );

        let string_error = guest.eval::<Vec<i64>>("'abc'").unwrap_err();
        let iterable_error = guest.eval::<Iter<B, i64>>("42").err().unwrap();

        assert!(string_error.is_conversion());
        assert!(string_error.to_string().contains("array"));
        assert!(iterable_error.is_conversion());
        assert!(iterable_error.to_string().contains("iterable"));
    }
}

guest_fixture! {
    pub fn rust_tuple_accepts_only_an_array_of_the_right_length<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert_eq!(
            guest.eval::<(i64, String)>("[1, 'a']").unwrap(),
            (1, "a".to_owned()),
        );

        for source in [
            "[1]",
            "[1, 'a', 3]",
            "({0: 1, 1: 'a', length: 2})",
        ] {
            assert!(guest
                .eval::<(i64, String)>(source)
                .is_err_and(|error| error.is_conversion()));
        }
    }
}

guest_fixture! {
    pub fn mapping_accepts_enumerable_own_properties<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert_eq!(
            Mappings::read(&guest, "({one: 1, two: 2})").unwrap(),
            vec![("one".to_owned(), 1), ("two".to_owned(), 2)],
        );
        assert_eq!(
            Mappings::read(&guest, "Object.freeze({two: 2, one: 1})").unwrap(),
            vec![("two".to_owned(), 2), ("one".to_owned(), 1)],
        );
        assert_eq!(
            Mappings::read(&guest, "({get three(){return 3;}, four: 4})").unwrap(),
            vec![("three".to_owned(), 3), ("four".to_owned(), 4)],
        );
        assert!(Mappings::read(&guest, "42")
            .is_err_and(|error| error.is_conversion()));

        for source in [
            "({get broken(){throw new RangeError('broken keys');}})",
            "new Proxy({}, {ownKeys(){throw new RangeError('broken keys');}})",
        ] {
            assert!(matches!(
                Mappings::read(&guest, source),
                Err(Error::Guest(exception))
                    if exception.matches("RangeError")
                        && exception.message() == "broken keys",
            ));
        }
    }
}

#[macro_export]
macro_rules! __guestjs_backend_values_tests {
    ($backend:ty) => {
        #[test]
        fn vec_reads_guest_arrays() {
            $crate::fixtures::values::vec_reads_guest_arrays::<$backend>();
        }

        #[test]
        fn rust_tuple_accepts_only_an_array_of_the_right_length() {
            $crate::fixtures::values::rust_tuple_accepts_only_an_array_of_the_right_length::<
                $backend,
            >();
        }

        #[test]
        fn mapping_accepts_enumerable_own_properties() {
            $crate::fixtures::values::mapping_accepts_enumerable_own_properties::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_values_tests as tests;
