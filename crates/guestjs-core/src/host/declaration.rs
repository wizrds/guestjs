use crate::{
    backend::{Backend, Val},
    enter::Enter,
    errors::Error,
    host::state::HostState,
};

pub trait Declaration<B: Backend> {
    fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
        name: &str,
    ) -> Result<Val<'js, B>, Error>;
}

#[cfg(test)]
mod tests {
    use crate::{backend::Backend, host::declaration::Declaration};

    #[allow(dead_code)]
    fn object_safe<B: Backend>(_: &dyn Declaration<B>) {}
}
