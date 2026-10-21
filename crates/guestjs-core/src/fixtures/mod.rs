pub mod backend;
pub mod buffers;
pub mod callables;
pub mod classes;
pub mod exceptions;
pub mod modules;
pub mod promises;
pub mod values;

macro_rules! guest_fixture {
    (
        pub fn $name:ident<$backend:ident>()
        where $bound:ident: [$first:path $(, $remaining:path)* $(,)?]
        using $builder:expr;
        |$guest:ident| $body:block
    ) => {
        pub fn $name<$backend>()
        where
            $backend: $first $(+ $remaining)*,
        {
            let $guest = $builder
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            $body
        }
    };
    (
        pub async fn $name:ident<$backend:ident>()
        where $bound:ident: [$first:path $(, $remaining:path)* $(,)?]
        using $builder:expr;
        |$guest:ident| $body:block
    ) => {
        pub async fn $name<$backend>()
        where
            $backend: $first $(+ $remaining)*,
        {
            let $guest = $builder
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            $body
        }
    };
}

pub(crate) use guest_fixture;
