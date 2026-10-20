use crate::{
    backend::values::{
        BackendValues,
        Kind,
    },
    errors::Error,
    handle::{
        class::Class,
        function::Function,
        object::Object,
        HasHandle,
    },
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

    fn object<K>(&self, key: K) -> Result<Object<B>, Error>
    where
        K: ToGuest<B>,
    {
        self.get::<Object<B>, _>(key)
    }

    fn function<K>(&self, key: K) -> Result<Function<B>, Error>
    where
        K: ToGuest<B>,
    {
        self.get::<Function<B>, _>(key)
    }

    fn class<K>(&self, key: K) -> Result<Class<B>, Error>
    where
        K: ToGuest<B>,
    {
        self.get::<Class<B>, _>(key)
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

    fn is_instance_of<R>(&self, class: &Class<B, R>) -> Result<bool, Error> {
        self.handle().with_enter(|enter, object| {
            B::instance_of(enter.token(), object, &class.to_guest(enter)?)
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

    fn call_method<K, A, R>(
        &self,
        key: K,
        args: A,
    ) -> Result<R::Owned, Error>
    where
        K: ToGuest<B>,
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
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
    fn call_this<T, A, R>(&self, this: T, args: A) -> Result<R::Owned, Error>
    where
        T: ToGuest<B>,
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
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

    fn call<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.call_this::<_, _, R>((), args)
    }
}

pub trait ConstructorProtocol<B: BackendValues>: HasHandle<B> {
    fn construct<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
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

    fn is_subclass_of<R>(&self, class: &Class<B, R>) -> Result<bool, Error> {
        self.handle().with_enter(|enter, constructor| {
            let base = class.to_guest(enter)?;

            Ok(B::same(enter.token(), constructor, &base)
                || B::instance_of(
                    enter.token(),
                    &B::get(
                        enter.token(),
                        constructor,
                        &"prototype".to_guest(enter)?,
                    )?,
                    &base,
                )?)
        })
    }
}

pub trait ValueProtocol<B: BackendValues>: HasHandle<B> {
    fn kind(&self) -> Result<Kind, Error> {
        self.handle()
            .with_enter(|enter, value| Ok(B::kind(enter.token(), value)))
    }

    fn truthy(&self) -> Result<bool, Error> {
        self.handle()
            .with_enter(|enter, value| B::truthy(enter.token(), value))
    }

    fn display(&self) -> Result<String, Error> {
        self.handle()
            .with_enter(|enter, value| B::display(enter.token(), value))
    }
}

impl<B: BackendValues, T: HasHandle<B>> ValueProtocol<B> for T {}
