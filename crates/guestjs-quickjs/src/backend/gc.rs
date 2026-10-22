use guestjs_core::{backend::gc::BackendGc, errors::Error};

use crate::backend::QuickJs;

impl BackendGc for QuickJs {
    fn run_gc(engine: &Self::Engine) -> Result<(), Error> {
        engine.run_gc()
    }
}
