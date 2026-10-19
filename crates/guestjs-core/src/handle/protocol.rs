use crate::{
    backend::values::BackendValues,
    errors::Error,
    handle::HasHandle,
    marshal::{
        FromGuest,
        ToGuest,
        ToGuestArgs,
    },
};

pub trait ObjectProtocol<B: BackendValues>: HasHandle<B> {
    fn cast<T>(&self) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            T::from_guest(enter, object.clone())
        })
    }

    fn get<T, K>(&self, key: K) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
        K: ToGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            T::from_guest(
                enter,
                B::get(enter.token(), object, &key.to_guest(enter)?)?,
            )
        })
    }

    fn set<K, V>(&self, key: K, value: V) -> Result<(), Error>
    where
        K: ToGuest<B>,
        V: ToGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            B::set(
                enter.token(),
                object,
                &key.to_guest(enter)?,
                value.to_guest(enter)?,
            )
        })
    }

    fn delete<K>(&self, key: K) -> Result<(), Error>
    where
        K: ToGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            B::delete(enter.token(), object, &key.to_guest(enter)?)
        })
    }

    fn has<K>(&self, key: K) -> Result<bool, Error>
    where
        K: ToGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            B::has(enter.token(), object, &key.to_guest(enter)?)
        })
    }

    fn keys<K>(&self) -> Result<Vec<K::Owned>, Error>
    where
        K: FromGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            B::keys(enter.token(), object)?
                .into_iter()
                .map(|key| K::from_guest(enter, key))
                .collect()
        })
    }

    fn call_method<R, K, A>(
        &self,
        key: K,
        args: A,
    ) -> Result<R::Owned, Error>
    where
        R: FromGuest<B>,
        K: ToGuest<B>,
        A: ToGuestArgs<B>,
    {
        self.handle().with_enter(|enter, object| {
            R::from_guest(
                enter,
                B::call(
                    enter.token(),
                    &B::get(
                        enter.token(),
                        object,
                        &key.to_guest(enter)?,
                    )?,
                    object,
                    &args.to_guest_args(enter)?,
                )?,
            )
        })
    }
}

pub trait CallableProtocol<B: BackendValues>: HasHandle<B> {
    fn call<R, T, A>(&self, this: T, args: A) -> Result<R::Owned, Error>
    where
        R: FromGuest<B>,
        T: ToGuest<B>,
        A: ToGuestArgs<B>,
    {
        self.handle().with_enter(|enter, function| {
            R::from_guest(
                enter,
                B::call(
                    enter.token(),
                    function,
                    &this.to_guest(enter)?,
                    &args.to_guest_args(enter)?,
                )?,
            )
        })
    }
}

pub trait ConstructorProtocol<B: BackendValues>: HasHandle<B> {
    fn construct<R, A>(&self, args: A) -> Result<R::Owned, Error>
    where
        R: FromGuest<B>,
        A: ToGuestArgs<B>,
    {
        self.handle().with_enter(|enter, constructor| {
            R::from_guest(
                enter,
                B::construct(
                    enter.token(),
                    constructor,
                    &args.to_guest_args(enter)?,
                )?,
            )
        })
    }
}
