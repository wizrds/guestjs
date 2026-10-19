use crate::{
    backend::{
        values::{
            BackendValues,
            Kind,
            WellKnown,
        },
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    handle::{
        base::Handle,
        protocol::ObjectProtocol,
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

pub struct Module<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Clone for Module<B> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for Module<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for Module<B> {}

impl<B: Backend> Describe for Module<B> {
    fn describe(expected: &mut Expected) {
        expected.push("module namespace");
    }
}

impl<B: BackendValues> FromGuest<B> for Module<B> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Object => match Option::<String>::from_guest(
                enter,
                B::get(
                    enter.token(),
                    &value,
                    &B::well_known(enter.token(), WellKnown::ToStringTag),
                )?,
            )?
            .as_deref()
            {
                Some("Module") => Ok(Self {
                    handle: Handle::from_value(enter, value),
                }),
                _ => Err(Expected::of::<Self>().mismatch(Kind::Object)),
            },
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for Module<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Module<B> {
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
        handle::{
            module::Module,
            protocol::ObjectProtocol,
        },
        marshal::{
            describe::Expected,
            FromGuest,
            ToGuest,
        },
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendValues>() {
        fn object<B: BackendValues, T: ObjectProtocol<B>>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

        object::<B, Module<B>>();
        from::<B, Module<B>>();
        to::<B, Module<B>>();
        to::<B, &Module<B>>();
    }

    #[test]
    fn description_names_a_module_namespace() {
        assert_eq!(
            Expected::of::<Module<Stub>>().to_string(),
            "module namespace",
        );
    }
}
