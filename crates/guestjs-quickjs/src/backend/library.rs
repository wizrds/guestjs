use std::ptr;

use guestjs_core::{
    backend::{Tok, library::BackendLibrary},
    errors::Error,
};
use rquickjs::{
    Module,
    module::{ModuleDef, ModuleLoadFn},
    qjs,
};

use crate::{backend::QuickJs, error::Capture};

#[derive(Clone, Copy)]
pub struct NativeModule {
    pub init: ModuleLoadFn,
}

impl NativeModule {
    pub fn new<D>() -> Self
    where
        D: ModuleDef,
    {
        Self { init: Self::init::<D> }
    }

    pub unsafe extern "C" fn init<D>(
        ctx: *mut qjs::JSContext,
        name: *const qjs::c_char,
    ) -> *mut qjs::JSModuleDef
    where
        D: ModuleDef,
    {
        match unsafe { Module::init_raw::<D>(ctx, name) } {
            module if module.is_null() || unsafe { qjs::JS_HasException(ctx) } => ptr::null_mut(),
            module => match unsafe {
                qjs::JS_ResolveModule(ctx, qjs::JS_MKPTR(qjs::JS_TAG_MODULE, module.cast()))
            } {
                0 => module,
                _ => ptr::null_mut(),
            },
        }
    }
}

impl BackendLibrary for QuickJs {
    type NativeModule = NativeModule;

    fn declare_native<'js>(
        token: Tok<'js, Self>,
        native: &Self::NativeModule,
        name: &str,
    ) -> Result<Self::Module<'js>, Error> {
        unsafe { Module::from_load_fn(token.ctx(), name, native.init) }.capture(token)
    }
}

#[cfg(test)]
mod tests {
    use guestjs_core::{
        backend::{Backend, library::BackendLibrary, modules::BackendModules},
        errors::Error,
    };
    use rquickjs::{
        Ctx,
        module::{Declarations, Exports, ModuleDef},
    };

    use crate::{
        backend::{QuickJs, library::NativeModule},
        engine::Config,
    };

    pub struct Greeting;

    impl ModuleDef for Greeting {
        fn declare<'js>(declarations: &Declarations<'js>) -> rquickjs::Result<()> {
            declarations
                .declare("greeting")
                .map(|_| ())
        }

        fn evaluate<'js>(_ctx: &Ctx<'js>, exports: &Exports<'js>) -> rquickjs::Result<()> {
            exports
                .export("greeting", "hello")
                .map(|_| ())
        }
    }

    pub struct Twice;

    impl ModuleDef for Twice {
        fn declare<'js>(declarations: &Declarations<'js>) -> rquickjs::Result<()> {
            declarations
                .declare("value")?
                .declare("value")
                .map(|_| ())
        }
    }

    #[test]
    fn native_module_declares_and_evaluates() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let (namespace, _) = QuickJs::evaluate(
                token,
                QuickJs::declare_native(token, &NativeModule::new::<Greeting>(), "greeting")
                    .unwrap(),
            )
            .unwrap();

            assert_eq!(
                namespace
                    .as_object()
                    .unwrap()
                    .get::<_, String>("greeting")
                    .unwrap(),
                "hello",
            );
        });
    }

    #[test]
    fn duplicate_native_export_is_captured() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(matches!(
                QuickJs::declare_native(token, &NativeModule::new::<Twice>(), "twice"),
                Err(Error::Guest(exception)) if exception.name() == "SyntaxError",
            ));
            assert!(!token.ctx().has_exception());
        });
    }
}
