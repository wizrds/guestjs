use crate::{
    backend::{
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    handle::{
        base::Handle,
        HasHandle,
    },
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        ToGuest,
    },
};

pub struct Value<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Value<B> {
    pub fn as_type<T>(&self) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        self.handle
            .with_enter(|enter, value| T::from_guest(enter, value.clone()))
    }
}

impl<B: Backend> Clone for Value<B> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for Value<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: Backend> Describe for Value<B> {
    fn describe(expected: &mut Expected) {
        expected.push("value");
    }
}

impl<B: Backend> FromGuest<B> for Value<B> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        Ok(Self {
            handle: Handle::from_value(enter, value),
        })
    }
}

impl<B: Backend> ToGuest<B> for Value<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Value<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{
            tests::Stub,
            values::BackendValues,
        },
        guest::Guest,
        handle::{
            class::Class,
            iter::Iter,
            object::Object,
            protocol::ValueProtocol,
            value::Value,
            HasHandle,
        },
        marshal::{
            describe::Expected,
            FromGuest,
            ToGuest,
        },
        runtime::Runtime,
    };

    #[allow(dead_code)]
    fn value_capabilities<B: BackendValues>() {
        fn value<B: BackendValues, T: ValueProtocol<B>>() {}

        value::<B, Value<B>>();
        value::<B, Object<B>>();
        value::<B, Class<B>>();
        value::<B, Iter<B, Value<B>>>();
    }

    #[test]
    fn conversion_retains_the_guest() {
        let guest = Guest::new(&Runtime::<Stub>::new(()).unwrap()).unwrap();
        let value = guest
            .enter(|enter| Value::from_guest(enter, ()))
            .unwrap();

        assert_eq!(value.handle().guest().id(), guest.id());
        assert_eq!(
            value
                .as_type::<Value<Stub>>()
                .unwrap()
                .handle()
                .guest()
                .id(),
            guest.id(),
        );
    }

    #[test]
    fn owned_and_borrowed_values_convert_back() {
        let guest = Guest::new(&Runtime::<Stub>::new(()).unwrap()).unwrap();
        let value = guest
            .enter(|enter| Value::from_guest(enter, ()))
            .unwrap();

        guest
            .enter(|enter| {
                assert!((&value).to_guest(enter).is_ok());
                assert!(value.clone().to_guest(enter).is_ok());

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn description_names_an_unrestricted_value() {
        assert_eq!(Expected::of::<Value<Stub>>().to_string(), "value");
    }
}
