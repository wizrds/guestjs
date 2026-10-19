pub mod base;
pub mod class;
pub mod function;
pub mod iter;
pub mod module;
pub mod object;
pub mod promise;
pub mod protocol;
pub mod value;

use crate::{
    backend::Backend,
    handle::base::Handle,
};

pub trait HasHandle<B: Backend> {
    fn handle(&self) -> &Handle<B>;
}
