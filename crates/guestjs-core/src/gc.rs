use crate::{backend::gc::BackendGc, errors::Error, runtime::Runtime};

pub trait RuntimeGcExt<B: BackendGc> {
    fn run_gc(&self) -> Result<(), Error>;
}

impl<B: BackendGc> RuntimeGcExt<B> for Runtime<B> {
    fn run_gc(&self) -> Result<(), Error> {
        B::run_gc(self.engine())
    }
}
