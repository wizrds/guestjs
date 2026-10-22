use crate::{
    backend::{
        buffers::BackendBuffers,
        values::{BackendValues, Kind},
    },
    errors::Error,
    handle::{HasHandle, buffer::ArrayBuffer, class::Class, function::Function, object::Object},
    marshal::{FromGuest, FromGuestMut, FromGuestRef, ToGuest, ToGuestArgs},
};

pub trait ObjectProtocol<B: BackendValues>: HasHandle<B> {
    fn prototype(&self) -> Result<Option<Object<B>>, Error> {
        self.handle()
            .with_enter(|enter, object| {
                Option::<Object<B>>::from_guest(enter, B::prototype_of(enter.token(), object)?)
            })
    }

    fn is_instance_of<R>(&self, class: &Class<B, R>) -> Result<bool, Error> {
        self.handle()
            .with_enter(|enter, object| {
                B::instance_of(enter.token(), object, &class.to_guest(enter)?)
            })
    }

    fn cast<T>(&self) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        self.handle()
            .with_enter(|enter, object| T::from_guest(enter, object.clone()))
    }

    fn get<T, K>(&self, key: K) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
        K: ToGuest<B>,
    {
        self.handle()
            .with_enter(|enter, object| {
                T::from_guest(enter, B::get(enter.token(), object, &key.to_guest(enter)?)?)
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
        self.handle()
            .with_enter(|enter, object| {
                B::set(enter.token(), object, &key.to_guest(enter)?, value.to_guest(enter)?)
            })
    }

    fn delete<K>(&self, key: K) -> Result<(), Error>
    where
        K: ToGuest<B>,
    {
        self.handle()
            .with_enter(|enter, object| B::delete(enter.token(), object, &key.to_guest(enter)?))
    }

    fn has<K>(&self, key: K) -> Result<bool, Error>
    where
        K: ToGuest<B>,
    {
        self.handle()
            .with_enter(|enter, object| B::has(enter.token(), object, &key.to_guest(enter)?))
    }

    fn keys<K>(&self) -> Result<Vec<K::Owned>, Error>
    where
        K: FromGuest<B>,
    {
        self.handle()
            .with_enter(|enter, object| {
                B::keys(enter.token(), object)?
                    .into_iter()
                    .map(|key| K::from_guest(enter, key))
                    .collect()
            })
    }

    fn call_method<K, A, R>(&self, key: K, args: A) -> Result<R::Owned, Error>
    where
        K: ToGuest<B>,
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.handle()
            .with_enter(|enter, object| {
                R::from_guest(
                    enter,
                    B::call(
                        enter.token(),
                        &B::get(enter.token(), object, &key.to_guest(enter)?)?,
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
        self.handle()
            .with_enter(|enter, function| {
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

pub trait BufferProtocol<B: BackendBuffers>: ValueProtocol<B> {
    fn byte_length(&self) -> Result<usize, Error> {
        self.handle()
            .with_enter(|enter, value| B::byte_length(enter.token(), value))
    }

    fn bytes(&self) -> Result<Vec<u8>, Error> {
        self.borrow_as_with::<[u8], _, _>(<[u8]>::to_vec)
    }

    fn write_bytes(&self, byte_offset: usize, bytes: &[u8]) -> Result<(), Error> {
        self.borrow_as_with_mut::<[u8], _, _>(|target| {
            match byte_offset
                .checked_add(bytes.len())
                .filter(|end| *end <= target.len())
            {
                Some(end) => {
                    target[byte_offset..end].copy_from_slice(bytes);

                    Ok(())
                }
                None => Err(Error::conversion(
                    format!(
                        "at most {} bytes at byte offset {byte_offset}",
                        target.len().saturating_sub(byte_offset),
                    ),
                    format!("{} bytes", bytes.len()),
                )),
            }
        })?
    }
}

pub trait ViewProtocol<B: BackendBuffers>: BufferProtocol<B> {
    fn buffer(&self) -> Result<ArrayBuffer<B>, Error> {
        self.handle().with_enter(|enter, view| {
            ArrayBuffer::from_guest(enter, B::buffer(enter.token(), view)?)
        })
    }

    fn byte_offset(&self) -> Result<usize, Error> {
        self.handle()
            .with_enter(|enter, view| B::byte_offset(enter.token(), view))
    }
}

pub trait ConstructorProtocol<B: BackendValues>: HasHandle<B> {
    fn construct<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.handle()
            .with_enter(|enter, constructor| {
                R::from_guest(
                    enter,
                    B::construct(enter.token(), constructor, &args.to_guest_args(enter)?)?,
                )
            })
    }

    fn is_subclass_of<R>(&self, class: &Class<B, R>) -> Result<bool, Error> {
        self.handle()
            .with_enter(|enter, constructor| {
                let base = class.to_guest(enter)?;

                Ok(B::same(enter.token(), constructor, &base)
                    || B::instance_of(
                        enter.token(),
                        &B::get(enter.token(), constructor, &"prototype".to_guest(enter)?)?,
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

    fn borrow_as_with<T, F, R>(&self, f: F) -> Result<R, Error>
    where
        T: FromGuestRef<B> + ?Sized,
        F: FnOnce(&T) -> R,
    {
        self.handle()
            .with_enter(|enter, value| T::from_guest_ref(enter, value).map(|borrowed| f(&borrowed)))
    }

    fn borrow_as_with_mut<T, F, R>(&self, f: F) -> Result<R, Error>
    where
        T: FromGuestMut<B> + ?Sized,
        F: FnOnce(&mut T) -> R,
    {
        self.handle()
            .with_enter(|enter, value| {
                T::from_guest_mut(enter, value).map(|mut borrowed| f(&mut borrowed))
            })
    }
}

impl<B: BackendValues, T: HasHandle<B>> ValueProtocol<B> for T {}
