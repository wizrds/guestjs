use rquickjs::Value as JsValue;

use crate::{
    errors::Error,
    marshal::{FromGuest, FromGuestBound, FromGuestMut, FromGuestRef},
    runtime::Scope,
};

/// Arguments passed from guest code into a host callable.
pub struct Args<'js> {
    values: Vec<JsValue<'js>>,
}

impl<'js> Args<'js> {
    pub(crate) fn new(values: Vec<JsValue<'js>>) -> Self {
        Self { values }
    }

    /// Returns the number of arguments.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether there are no arguments.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns an argument.
    pub fn get<T>(&self, scope: &Scope<'js>, index: usize) -> Result<T::Bound<'js>, Error>
    where
        T: FromGuestBound,
    {
        self.get_opt::<T>(scope, index)?
            .ok_or_else(|| Error::conversion(format!("missing host argument at index {index}")))
    }

    /// Returns an optional argument.
    pub fn get_opt<T>(
        &self,
        scope: &Scope<'js>,
        index: usize,
    ) -> Result<Option<T::Bound<'js>>, Error>
    where
        T: FromGuestBound,
    {
        match self.values.get(index).cloned() {
            Some(value) => T::from_guest_bound(scope, value).map(Some),
            None => Ok(None),
        }
    }

    /// Returns an argument in its owned form, for capture by a future outliving the callback.
    pub fn get_owned<T>(&self, scope: &Scope<'js>, index: usize) -> Result<T::Owned, Error>
    where
        T: FromGuest,
    {
        T::from_guest(scope, self.required(index)?)
    }

    /// Returns a shared argument borrow.
    pub fn get_borrow<C>(&self, scope: &Scope<'js>, index: usize) -> Result<C::Ref, Error>
    where
        C: FromGuestRef<'js>,
    {
        C::from_guest_ref(scope, self.required(index)?)
    }

    /// Returns an exclusive argument borrow.
    pub fn get_borrow_mut<C>(&self, scope: &Scope<'js>, index: usize) -> Result<C::Mut, Error>
    where
        C: FromGuestMut<'js>,
    {
        C::from_guest_mut(scope, self.required(index)?)
    }

    /// Returns arguments from an index.
    pub fn get_rest<T>(&self, scope: &Scope<'js>, index: usize) -> Result<Vec<T::Bound<'js>>, Error>
    where
        T: FromGuestBound,
    {
        self.values
            .iter()
            .skip(index)
            .cloned()
            .map(|value| T::from_guest_bound(scope, value))
            .collect()
    }

    fn required(&self, index: usize) -> Result<JsValue<'js>, Error> {
        self.values
            .get(index)
            .cloned()
            .ok_or_else(|| Error::conversion(format!("missing host argument at index {index}")))
    }
}

#[cfg(test)]
mod tests {
    use crate::{host::args::Args, marshal::ToGuestBound, runtime::Runtime};

    #[tokio::test]
    async fn returns_arguments_from_starting_index() {
        Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .scope(async |scope| {
                let args = Args::new(vec![
                    1_i32.to_guest_bound(&scope)?,
                    2_i32.to_guest_bound(&scope)?,
                    3_i32.to_guest_bound(&scope)?,
                ]);

                assert_eq!(args.get_rest::<i32>(&scope, 0)?, vec![1, 2, 3]);
                assert_eq!(args.get_rest::<i32>(&scope, 1)?, vec![2, 3]);
                assert_eq!(args.get_rest::<i32>(&scope, 3)?, Vec::<i32>::new());

                Ok(())
            })
            .await
            .unwrap();
    }
}
