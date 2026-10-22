use crate::{backend::Backend, errors::Error};

pub trait BackendGc: Backend {
    fn run_gc(engine: &Self::Engine) -> Result<(), Error>;
}
