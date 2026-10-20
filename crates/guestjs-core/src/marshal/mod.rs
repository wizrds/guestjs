pub mod args;
pub mod buffers;
pub mod collections;
pub mod describe;
pub mod primitives;

#[cfg(feature = "serde")]
pub mod serde;

use crate::{
    backend::{
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
};

pub trait ToGuest<B: Backend> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error>;
}

pub trait FromGuest<B: Backend> {
    type Owned: 'static;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error>;
}

pub trait FromGuestRef<B: Backend> {
    type Ref<'a>;

    fn from_guest_ref<'js, 'a>(
        enter: &Enter<'js, B>,
        value: &'a Val<'js, B>,
    ) -> Result<Self::Ref<'a>, Error>;
}

pub trait FromGuestMut<B: Backend> {
    type Mut<'a>;

    fn from_guest_mut<'js, 'a>(
        enter: &Enter<'js, B>,
        value: &'a Val<'js, B>,
    ) -> Result<Self::Mut<'a>, Error>;
}

pub trait ToGuestArgs<B: Backend> {
    fn to_guest_args<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Vec<Val<'js, B>>, Error>;
}

#[cfg(test)]
mod tests {
    use std::collections::{
        BTreeMap,
        HashMap,
    };

    use crate::{
        backend::{
            values::BackendValues,
            Backend,
        },
        marshal::{
            FromGuest,
            ToGuest,
            ToGuestArgs,
        },
    };

    #[allow(dead_code)]
    fn conversions<B: BackendValues>() {
        fn to<B: Backend, T: ToGuest<B>>() {}
        fn from<B: Backend, T: FromGuest<B>>() {}
        fn args<B: Backend, T: ToGuestArgs<B>>() {}

        to::<B, ()>();
        from::<B, ()>();
        to::<B, bool>();
        from::<B, bool>();
        to::<B, i8>();
        from::<B, i8>();
        to::<B, i16>();
        from::<B, i16>();
        to::<B, i32>();
        from::<B, i32>();
        to::<B, i64>();
        from::<B, i64>();
        to::<B, isize>();
        from::<B, isize>();
        to::<B, u8>();
        from::<B, u8>();
        to::<B, u16>();
        from::<B, u16>();
        to::<B, u32>();
        from::<B, u32>();
        to::<B, u64>();
        from::<B, u64>();
        to::<B, usize>();
        from::<B, usize>();
        to::<B, f32>();
        from::<B, f32>();
        to::<B, f64>();
        from::<B, f64>();
        to::<B, char>();
        from::<B, char>();
        to::<B, &str>();
        to::<B, String>();
        from::<B, String>();
        to::<B, Option<i64>>();
        from::<B, Option<i64>>();
        to::<B, Vec<i64>>();
        from::<B, Vec<i64>>();
        to::<B, (i64, String)>();
        from::<B, (i64, String)>();
        to::<B, HashMap<String, i64>>();
        from::<B, HashMap<String, i64>>();
        to::<B, BTreeMap<String, i64>>();
        from::<B, BTreeMap<String, i64>>();
        args::<B, ()>();
        args::<B, (i64, String)>();
        args::<B, Vec<i64>>();
    }
}
