use std::{
    any::{Any, TypeId, type_name},
    cell::{self, RefCell},
    ptr,
};

use guestjs_core::{
    backend::{
        Tok, Val,
        callables::{BackendCallables, RawBody},
        classes::BackendClasses,
    },
    errors::{BorrowKind, Error},
};
use rquickjs::{
    Class, Constructor, Ctx, Function, JsLifetime, Object, Value,
    class::{JsClass, Readable, Trace, Tracer},
};

use crate::{backend::QuickJs, error::Capture};

pub struct PayloadCell {
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub cell: RefCell<Box<dyn Any>>,
}

impl PayloadCell {
    pub fn new<C>(payload: C) -> Self
    where
        C: 'static,
    {
        Self {
            type_id: TypeId::of::<C>(),
            type_name: type_name::<C>(),
            cell: RefCell::new(Box::new(payload)),
        }
    }

    pub fn of<'a, 'js>(value: &'a Value<'js>) -> Option<&'a Self> {
        value
            .as_object()
            .and_then(|object| object.as_class::<Self>())
            .map(|class| unsafe { &*ptr::from_ref(&*class.borrow()) })
    }

    pub fn typed<'a, 'js, C>(value: &'a Value<'js>) -> Result<&'a Self, Error>
    where
        C: 'static,
    {
        Self::of(value)
            .filter(|payload| payload.type_id == TypeId::of::<C>())
            .ok_or_else(|| {
                Error::conversion(
                    type_name::<C>(),
                    Self::of(value).map_or_else(|| value.type_name(), |payload| payload.type_name),
                )
            })
    }
}

unsafe impl<'js> JsLifetime<'js> for PayloadCell {
    type Changed<'to> = PayloadCell;
}

impl<'js> Trace<'js> for PayloadCell {
    fn trace<'a>(&self, _: Tracer<'a, 'js>) {}
}

impl<'js> JsClass<'js> for PayloadCell {
    const NAME: &'static str = "PayloadCell";

    type Mutable = Readable;

    fn constructor(_: &Ctx<'js>) -> rquickjs::Result<Option<Constructor<'js>>> {
        Ok(None)
    }
}

impl BackendClasses for QuickJs {
    type Payload<'a, C: 'static> = cell::Ref<'a, C>;
    type PayloadMut<'a, C: 'static> = cell::RefMut<'a, C>;

    fn new_constructor<'js>(
        token: Tok<'js, Self>,
        name: &str,
        length: u32,
        body: RawBody<Self>,
    ) -> Result<Val<'js, Self>, Error> {
        Self::new_function(token, name, length, body).and_then(|function| {
            Function::from_value(function)
                .map(|function| {
                    function
                        .with_constructor(true)
                        .into_value()
                })
                .capture(token)
        })
    }

    fn new_instance<'js, C: 'static>(
        token: Tok<'js, Self>,
        prototype: &Val<'js, Self>,
        payload: C,
    ) -> Result<Val<'js, Self>, Error> {
        Object::from_value(prototype.clone())
            .and_then(|prototype| Class::instance_proto(PayloadCell::new(payload), prototype))
            .map(Class::into_value)
            .capture(token)
    }

    fn payload_type<'js>(_token: Tok<'js, Self>, value: &Val<'js, Self>) -> Option<TypeId> {
        PayloadCell::of(value).map(|payload| payload.type_id)
    }

    fn borrow_payload<'js, 'a, C: 'static>(
        _token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::Payload<'a, C>, Error> {
        PayloadCell::typed::<C>(value).and_then(|payload| {
            payload
                .cell
                .try_borrow()
                .map(|borrowed| {
                    cell::Ref::map(borrowed, |any| {
                        any.downcast_ref::<C>()
                            .expect("payload type was checked")
                    })
                })
                .map_err(|_| Error::borrow(BorrowKind::Exclusive, payload.type_name))
        })
    }

    fn borrow_payload_mut<'js, 'a, C: 'static>(
        _token: Tok<'js, Self>,
        value: &'a Val<'js, Self>,
    ) -> Result<Self::PayloadMut<'a, C>, Error> {
        PayloadCell::typed::<C>(value).and_then(|payload| {
            payload
                .cell
                .try_borrow_mut()
                .map(|borrowed| {
                    cell::RefMut::map(borrowed, |any| {
                        any.downcast_mut::<C>()
                            .expect("payload type was checked")
                    })
                })
                .map_err(|_| {
                    Error::borrow(
                        payload
                            .cell
                            .try_borrow()
                            .map_or(BorrowKind::Exclusive, |_| BorrowKind::Shared),
                        payload.type_name,
                    )
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{any::TypeId, rc::Rc};

    use guestjs_core::{
        backend::{Backend, callables::RawCall, classes::BackendClasses, values::BackendValues},
        errors::{BorrowKind, Error},
    };
    use rquickjs::Value;

    use crate::{backend::QuickJs, engine::Config};

    pub struct Counter(pub f64);

    guestjs_core::fixtures::classes::tests!(QuickJs);

    #[test]
    fn constructor_builds_subclass_instances() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::set(
                token,
                &QuickJs::globals(token),
                &QuickJs::new_string(token, "Host").unwrap(),
                QuickJs::new_constructor(
                    token,
                    "Host",
                    1,
                    Rc::new(|call: RawCall<'_, QuickJs>| {
                        QuickJs::new_instance(
                            call.token,
                            &QuickJs::get(
                                call.token,
                                call.new_target.as_ref().unwrap(),
                                &QuickJs::new_string(call.token, "prototype")?,
                            )?,
                            Counter(QuickJs::as_f64(call.token, &call.args[0])?),
                        )
                    }),
                )
                .unwrap(),
            )
            .unwrap();

            let instance = token
                .ctx()
                .eval::<Value, _>(
                    r#"
                    Host.prototype = {};
                    class Derived extends Host {}
                    globalThis.instance = new Derived(3);
                    "#,
                )
                .unwrap();

            assert!(
                token
                    .ctx()
                    .eval::<bool, _>("instance instanceof Derived && instance instanceof Host")
                    .unwrap()
            );
            assert_eq!(QuickJs::payload_type(token, &instance), Some(TypeId::of::<Counter>()),);

            QuickJs::borrow_payload_mut::<Counter>(token, &instance)
                .unwrap()
                .0 += 1.0;

            assert_eq!(
                QuickJs::borrow_payload::<Counter>(token, &instance)
                    .unwrap()
                    .0,
                4.0,
            );
        });
    }

    #[test]
    fn borrow_payload_reports_conflicts_and_mismatches() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let instance =
                QuickJs::new_instance(token, &QuickJs::new_object(token).unwrap(), Counter(0.0))
                    .unwrap();
            let plain = QuickJs::new_object(token).unwrap();

            {
                let _shared = QuickJs::borrow_payload::<Counter>(token, &instance).unwrap();

                assert!(matches!(
                    QuickJs::borrow_payload_mut::<Counter>(token, &instance),
                    Err(Error::Borrow { kind: BorrowKind::Shared, .. }),
                ));
            }

            {
                let _exclusive = QuickJs::borrow_payload_mut::<Counter>(token, &instance).unwrap();

                assert!(matches!(
                    QuickJs::borrow_payload::<Counter>(token, &instance),
                    Err(Error::Borrow { kind: BorrowKind::Exclusive, .. }),
                ));
                assert!(
                    QuickJs::borrow_payload::<String>(token, &instance)
                        .is_err_and(|error| error.is_conversion()),
                );
                assert_eq!(QuickJs::payload_type(token, &instance), Some(TypeId::of::<Counter>()),);
            }

            assert_eq!(QuickJs::payload_type(token, &plain), None);
            assert!(
                QuickJs::borrow_payload::<Counter>(token, &plain)
                    .is_err_and(|error| error.is_conversion()),
            );
            assert!(
                QuickJs::new_instance(token, &QuickJs::new_number(token, 1.0), Counter(0.0))
                    .is_err_and(|error| error.is_conversion()),
            );
        });
    }
}
