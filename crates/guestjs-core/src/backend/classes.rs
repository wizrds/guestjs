use std::{
    any::TypeId,
    ops::{Deref, DerefMut},
};

use crate::{
    backend::{
        Tok, Val,
        callables::{BackendCallables, RawBody},
    },
    errors::Error,
};

pub trait BackendClasses: BackendCallables {
    type Payload<'a, C: 'static>: Deref<Target = C> + 'a;
    type PayloadMut<'a, C: 'static>: DerefMut<Target = C> + 'a;

    fn new_constructor<'js>(
        token: Tok<'js, Self>,
        name: &str,
        length: u32,
        body: RawBody<Self>,
    ) -> Result<Val<'js, Self>, Error>;

    fn new_instance<'js, C: 'static>(
        token: Tok<'js, Self>,
        prototype: &Val<'js, Self>,
        payload: C,
    ) -> Result<Val<'js, Self>, Error>;

    fn payload_type<'js>(token: Tok<'js, Self>, value: &Val<'js, Self>) -> Option<TypeId>;

    fn borrow_payload<'js, 'a, C: 'static>(
        token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::Payload<'a, C>, Error>;

    fn borrow_payload_mut<'js, 'a, C: 'static>(
        token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::PayloadMut<'a, C>, Error>;
}
