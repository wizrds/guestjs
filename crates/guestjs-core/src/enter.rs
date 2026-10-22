use crate::{
    backend::{Backend, Tok},
    guest::Guest,
};

pub struct Enter<'js, B: Backend> {
    token: Tok<'js, B>,
    guest: Guest<B>,
}

impl<'js, B: Backend> Enter<'js, B> {
    pub(crate) fn new(token: Tok<'js, B>, guest: Guest<B>) -> Self {
        Self { token, guest }
    }

    pub fn token(&self) -> Tok<'js, B> {
        self.token
    }

    pub fn guest(&self) -> &Guest<B> {
        &self.guest
    }
}
