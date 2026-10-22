use crate::{
    backend::{Backend, Val},
    enter::Enter,
    errors::Error,
    host::{declaration::Declaration, state::HostState},
    marshal::ToGuest,
};

pub struct ValueDeclaration<T> {
    value: T,
}

impl<T> ValueDeclaration<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

impl<B, T> Declaration<B> for ValueDeclaration<T>
where
    B: Backend,
    T: ToGuest<B> + Clone,
{
    fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        _state: &HostState,
        _name: &str,
    ) -> Result<Val<'js, B>, Error> {
        self.value.clone().to_guest(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::values::BackendValues,
        host::{declaration::Declaration, value::ValueDeclaration},
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendValues>() {
        fn declaration<B: BackendValues, T: Declaration<B>>() {}

        declaration::<B, ValueDeclaration<f64>>();
        declaration::<B, ValueDeclaration<String>>();
    }
}
