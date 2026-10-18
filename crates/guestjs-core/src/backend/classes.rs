use std::{
    any::TypeId,
    ops::{
        Deref,
        DerefMut,
    },
};

use crate::{
    backend::{
        callables::{
            BackendCallables,
            RawBody,
        },
        Tok,
        Val,
    },
    errors::Error,
};

pub trait BackendClasses: BackendCallables {
    type Ref<'a, C: 'static>: Deref<Target = C> + 'a;
    type RefMut<'a, C: 'static>: DerefMut<Target = C> + 'a;

    fn constructor<'js>(
        token: Tok<'js, Self>,
        name: &str,
        length: u32,
        body: RawBody<Self>,
    ) -> Result<Val<'js, Self>, Error>;

    fn instantiate<'js, C: 'static>(
        token: Tok<'js, Self>,
        prototype: &Val<'js, Self>,
        payload: C,
    ) -> Result<Val<'js, Self>, Error>;

    fn payload_type<'js>(
        token: Tok<'js, Self>,
        value: &Val<'js, Self>,
    ) -> Option<TypeId>;

    fn borrow<'js, 'a, C: 'static>(
        token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::Ref<'a, C>, Error>;

    fn borrow_mut<'js, 'a, C: 'static>(
        token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::RefMut<'a, C>, Error>;
}
