use std::cell::Cell;

use crate::{
    backend::{
        callables::RawCall,
        values::BackendValues,
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    marshal::{
        FromGuest,
        FromGuestMut,
        FromGuestRef,
        ToGuest,
        ToGuestArgs,
    },
};

pub struct Args<'js, B: Backend> {
    this: Val<'js, B>,
    values: Vec<Val<'js, B>>,
    new_target: Val<'js, B>,
    undefined: Val<'js, B>,
    next: Cell<usize>,
}

impl<'js, B: BackendValues> Args<'js, B> {
    pub fn new(call: RawCall<'js, B>) -> Self {
        Self {
            this: call.this,
            values: call.args,
            new_target: call
                .new_target
                .unwrap_or_else(|| B::undefined(call.token)),
            undefined: B::undefined(call.token),
            next: Cell::new(0),
        }
    }

    pub fn advance(&self) -> &Val<'js, B> {
        self.values
            .get(self.next.replace(self.next.get() + 1))
            .unwrap_or(&self.undefined)
    }

    pub fn this<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        T::from_guest(enter, self.this.clone())
    }

    pub fn this_ref<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Ref<'_>, Error>
    where
        T: FromGuestRef<B>,
    {
        T::from_guest_ref(enter, &self.this)
    }

    pub fn this_mut<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Mut<'_>, Error>
    where
        T: FromGuestMut<B>,
    {
        T::from_guest_mut(enter, &self.this)
    }

    pub fn new_target<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        T::from_guest(enter, self.new_target.clone())
    }

    pub fn next<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        T::from_guest(enter, self.advance().clone())
    }

    pub fn next_ref<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Ref<'_>, Error>
    where
        T: FromGuestRef<B>,
    {
        T::from_guest_ref(enter, self.advance())
    }

    pub fn next_mut<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<T::Mut<'_>, Error>
    where
        T: FromGuestMut<B>,
    {
        T::from_guest_mut(enter, self.advance())
    }

    pub fn rest<T>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<Vec<T::Owned>, Error>
    where
        T: FromGuest<B>,
    {
        self.values
            .get(self.next.replace(self.values.len())..)
            .unwrap_or_default()
            .iter()
            .map(|value| T::from_guest(enter, value.clone()))
            .collect()
    }
}

macro_rules! tuple {
    ($($name:ident $value:ident),+) => {
        impl<B, $($name),+> ToGuestArgs<B> for ($($name,)+)
        where
            B: Backend,
            $($name: ToGuest<B>,)+
        {
            fn to_guest_args<'js>(
                self,
                enter: &Enter<'js, B>,
            ) -> Result<Vec<Val<'js, B>>, Error> {
                let ($($value,)+) = self;

                Ok(vec![$($value.to_guest(enter)?),+])
            }
        }
    };
}

tuple!(A a);
tuple!(A a, B2 b);
tuple!(A a, B2 b, C c);
tuple!(A a, B2 b, C c, D d);
tuple!(A a, B2 b, C c, D d, E e);
tuple!(A a, B2 b, C c, D d, E e, F f);
tuple!(A a, B2 b, C c, D d, E e, F f, G g);
tuple!(A a, B2 b, C c, D d, E e, F f, G g, H h);
tuple!(A a, B2 b, C c, D d, E e, F f, G g, H h, I i);
tuple!(A a, B2 b, C c, D d, E e, F f, G g, H h, I i, J j);
tuple!(A a, B2 b, C c, D d, E e, F f, G g, H h, I i, J j, K k);
tuple!(A a, B2 b, C c, D d, E e, F f, G g, H h, I i, J j, K k, L l);

impl<B: Backend> ToGuestArgs<B> for () {
    fn to_guest_args<'js>(
        self,
        _enter: &Enter<'js, B>,
    ) -> Result<Vec<Val<'js, B>>, Error> {
        Ok(Vec::new())
    }
}

impl<B, T> ToGuestArgs<B> for Vec<T>
where
    B: Backend,
    T: ToGuest<B>,
{
    fn to_guest_args<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Vec<Val<'js, B>>, Error> {
        self.into_iter()
            .map(|value| value.to_guest(enter))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::tests::Stub,
        guest::Guest,
        marshal::ToGuestArgs,
        runtime::Runtime,
    };

    #[test]
    fn unit_passes_no_arguments() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();

        assert_eq!(
            guest
                .enter(|enter| {
                    ().to_guest_args(enter)
                        .map(|values| values.len())
                })
                .unwrap(),
            0,
        );
    }
}
