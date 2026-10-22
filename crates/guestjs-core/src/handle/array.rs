use crate::{
    backend::{
        Backend, Val,
        values::{BackendValues, Kind},
    },
    enter::Enter,
    errors::Error,
    handle::{HasHandle, base::Handle, protocol::ObjectProtocol},
    marshal::{
        FromGuest, ToGuest, ToGuestArgs,
        describe::{Describe, Expected},
    },
};

pub struct Array<B: Backend> {
    handle: Handle<B>,
}

impl<B: BackendValues> Array<B> {
    pub fn from_values<A>(enter: &Enter<'_, B>, values: A) -> Result<Self, Error>
    where
        A: ToGuestArgs<B>,
    {
        Self::from_guest(enter, B::new_array(enter.token(), values.to_guest_args(enter)?)?)
    }

    pub fn len(&self) -> Result<usize, Error> {
        self.get::<usize, _>("length")
    }

    pub fn is_empty(&self) -> Result<bool, Error> {
        self.len().map(|len| len == 0)
    }

    pub fn to_vec<T>(&self) -> Result<Vec<T::Owned>, Error>
    where
        T: FromGuest<B>,
    {
        self.cast::<Vec<T>>()
    }
}

impl<B: Backend> Clone for Array<B> {
    fn clone(&self) -> Self {
        Self { handle: self.handle.clone() }
    }
}

impl<B: Backend> HasHandle<B> for Array<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for Array<B> {}

impl<B: Backend> Describe for Array<B> {
    fn describe(expected: &mut Expected) {
        expected.push("array");
    }
}

impl<B: BackendValues> FromGuest<B> for Array<B> {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Array => Ok(Self { handle: Handle::from_value(enter, value) }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for Array<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Array<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{tests::Stub, values::BackendValues},
        handle::{
            HasHandle,
            array::Array,
            protocol::{ObjectProtocol, ValueProtocol},
        },
        marshal::{FromGuest, ToGuest, describe::Expected},
    };

    #[test]
    fn description_names_an_array() {
        assert_eq!(Expected::of::<Array<Stub>>().to_string(), "array");
    }

    #[test]
    fn arrays_implement_the_handle_contracts() {
        fn contracts<B, T>()
        where
            B: BackendValues,
            T: Clone
                + HasHandle<B>
                + ObjectProtocol<B>
                + ValueProtocol<B>
                + FromGuest<B, Owned = T>
                + ToGuest<B>,
            for<'a> &'a T: ToGuest<B>,
        {
        }

        contracts::<Stub, Array<Stub>>();
    }
}
