use std::{
    collections::HashSet,
    ffi::CString,
    rc::Rc,
};

use futures::FutureExt;
use guestjs_core::{
    backend::{
        library::BackendLibrary,
        modules::{
            BackendModules,
            ModuleDispatcher,
        },
        Tok,
        Val,
    },
    errors::{
        Error,
        GuestErrorKind,
    },
};
use rquickjs::{
    context::EvalOptions,
    loader::{
        ImportAttributes,
        Loader,
        Resolver,
    },
    module::{
        Declarations,
        Exports,
        ModuleDef,
    },
    Ctx,
    JsLifetime,
    Module,
    Value,
};

use crate::{
    backend::{
        library::NativeModule,
        QuickJs,
    },
    context::Token,
    error::{
        Capture,
        Create,
        Raise,
    },
};

pub struct PendingExports<'js> {
    pub exports: Vec<(CString, Value<'js>)>,
}

unsafe impl<'js> JsLifetime<'js> for PendingExports<'js> {
    type Changed<'to> = PendingExports<'to>;
}

pub struct Synthetic;

impl ModuleDef for Synthetic {
    fn declare<'js>(declarations: &Declarations<'js>) -> rquickjs::Result<()> {
        declarations
            .module()
            .ctx()
            .userdata::<PendingExports>()
            .ok_or(rquickjs::Error::Unknown)?
            .exports
            .iter()
            .try_for_each(|(name, _)| declarations.declare_c_str(name).map(|_| ()))
    }

    fn evaluate<'js>(
        ctx: &Ctx<'js>,
        exports: &Exports<'js>,
    ) -> rquickjs::Result<()> {
        ctx.remove_userdata::<PendingExports>()
            .ok()
            .flatten()
            .ok_or(rquickjs::Error::Unknown)?
            .exports
            .into_iter()
            .try_for_each(|(name, value)| exports.export_c_str(&name, value).map(|_| ()))
    }
}

pub struct Dispatcher {
    pub dispatcher: Rc<dyn ModuleDispatcher<QuickJs>>,
}

impl Resolver for Dispatcher {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let token = Token::from_ctx(ctx);

        match attributes.and_then(|attributes| attributes.keys().next()) {
            Some(Ok(key)) => Err(GuestErrorKind::SyntaxError.throw(
                token,
                &format!("import attribute '{key}' is not supported"),
            )),
            Some(Err(error)) => Err(error),
            None => self
                .dispatcher
                .resolve(token, base, name)
                .map_err(|error| error.raise(token)),
        }
    }
}

impl Loader for Dispatcher {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let token = Token::from_ctx(ctx);

        self.dispatcher
            .load(token, name)
            .map_err(|error| error.raise(token))
            .and_then(|module| {
                module
                    .name::<String>()
                    .and_then(|loaded| match loaded == name {
                        true => Ok(module),
                        false => Err(Error::unsupported(format!(
                            "module dispatcher loaded '{loaded}' for '{name}'"
                        ))
                        .raise(token)),
                    })
            })
    }
}

impl BackendModules for QuickJs {
    type Module<'js> = Module<'js>;

    fn eval<'js>(
        token: Tok<'js, Self>,
        source: &str,
        name: &str,
    ) -> Result<Val<'js, Self>, Error> {
        let mut options = EvalOptions::default();

        options.strict = false;
        options.filename = Some(name.to_string());

        token
            .ctx()
            .eval_with_options(source, options)
            .capture(token)
    }

    fn declare<'js>(
        token: Tok<'js, Self>,
        name: &str,
        source: &str,
    ) -> Result<Self::Module<'js>, Error> {
        Module::declare(token.ctx(), name, source).capture(token)
    }

    fn synthetic<'js>(
        token: Tok<'js, Self>,
        name: &str,
        exports: Vec<(String, Val<'js, Self>)>,
    ) -> Result<Self::Module<'js>, Error> {
        let exports = exports
            .into_iter()
            .map(|(name, value)| {
                CString::new(name)
                    .map(|name| (name, value))
                    .map_err(rquickjs::Error::InvalidString)
            })
            .collect::<rquickjs::Result<Vec<_>>>()
            .capture(token)?;

        let mut names = HashSet::new();

        if let Some((duplicate, _)) = exports
            .iter()
            .find(|(name, _)| !names.insert(name))
        {
            return Err(GuestErrorKind::SyntaxError.throw(
                token,
                &format!("duplicate exported name '{}'", duplicate.to_string_lossy()),
            ))
            .capture(token);
        }

        token
            .ctx()
            .store_userdata(PendingExports { exports })
            .map_err(|_| Error::engine("runtime userdata is borrowed"))?;

        Self::declare_native(token, &NativeModule::new::<Synthetic>(), name)
            .and_then(|module| {
                module
                    .clone()
                    .eval()
                    .and_then(|(_, promise)| promise.result::<()>().unwrap_or(Ok(())))
                    .map(|_| module)
                    .capture(token)
            })
            .inspect_err(|_| drop(token.ctx().remove_userdata::<PendingExports>()))
    }

    fn evaluate<'js>(
        token: Tok<'js, Self>,
        module: Self::Module<'js>,
    ) -> Result<(Val<'js, Self>, Val<'js, Self>), Error> {
        module
            .eval()
            .and_then(|(module, promise)| {
                module
                    .namespace()
                    .map(|namespace| (namespace.into_value(), promise.into_value()))
            })
            .capture(token)
    }

    fn install(
        engine: &Self::Engine,
        dispatcher: Rc<dyn ModuleDispatcher<Self>>,
    ) -> Result<(), Error> {
        if engine.is_entered() {
            return Err(Error::unsupported(
                "a module dispatcher cannot be installed inside an entry",
            ));
        }

        engine
            .runtime()
            .set_loader(
                Dispatcher {
                    dispatcher: dispatcher.clone(),
                },
                Dispatcher { dispatcher },
            )
            .now_or_never()
            .expect("runtime lock is held elsewhere");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use guestjs_core::{
        backend::{
            modules::{
                BackendModules,
                ModuleDispatcher,
            },
            values::BackendValues,
            Backend,
            Tok,
        },
        errors::Error,
    };
    use rquickjs::{
        promise::PromiseState,
        Object,
    };

    use crate::{
        backend::QuickJs,
        engine::Config,
    };

    pub struct Sources;

    impl ModuleDispatcher<QuickJs> for Sources {
        fn resolve<'js>(
            &self,
            _token: Tok<'js, QuickJs>,
            _referrer: &str,
            specifier: &str,
        ) -> Result<String, Error> {
            Ok(specifier.to_string())
        }

        fn load<'js>(
            &self,
            token: Tok<'js, QuickJs>,
            name: &str,
        ) -> Result<<QuickJs as BackendModules>::Module<'js>, Error> {
            match name {
                "numbers" => QuickJs::synthetic(
                    token,
                    name,
                    vec![("one".to_string(), QuickJs::number(token, 1.0))],
                ),
                "double" => QuickJs::declare(
                    token,
                    name,
                    r#"
                    import { one } from "numbers";

                    export const two = one * 2;
                    "#,
                ),
                "renamed" => QuickJs::declare(token, "other", "export {};"),
                _ => Err(Error::unsupported(format!(
                    "no module named '{name}'"
                ))),
            }
        }
    }

    #[test]
    fn eval_runs_sloppy_scripts() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert_eq!(
                QuickJs::eval(
                    token,
                    r#"
                    total = 1;

                    with ({ step: 2 }) {
                        total + step;
                    }
                    "#,
                    "script.js",
                )
                .unwrap()
                .as_number(),
                Some(3.0),
            );
            assert!(matches!(
                QuickJs::eval(token, "let let = 1;", "broken.js"),
                Err(Error::Guest(exception)) if exception.name() == "SyntaxError",
            ));
        });
    }

    #[test]
    fn evaluate_returns_namespace_and_one_promise() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let module = QuickJs::declare(
                token,
                "main",
                r#"
                globalThis.runs = (globalThis.runs ?? 0) + 1;

                export const answer = 42;
                "#,
            )
            .unwrap();

            let (namespace, promise) = QuickJs::evaluate(token, module.clone()).unwrap();
            let (_, again) = QuickJs::evaluate(token, module).unwrap();

            assert_eq!(
                namespace
                    .as_object()
                    .unwrap()
                    .get::<_, i32>("answer")
                    .unwrap(),
                42,
            );
            assert_eq!(promise.as_promise().unwrap().state(), PromiseState::Resolved);
            assert_eq!(promise, again);
            assert_eq!(
                QuickJs::eval(token, "runs", "runs.js")
                    .unwrap()
                    .as_number(),
                Some(1.0),
            );
        });
    }

    #[test]
    fn throwing_module_rejects_its_promise() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let (namespace, promise) = QuickJs::evaluate(
                token,
                QuickJs::declare(token, "throws", r#"throw new RangeError("boom");"#).unwrap(),
            )
            .unwrap();

            assert!(namespace.is_object());
            assert_eq!(promise.as_promise().unwrap().state(), PromiseState::Rejected);
        });
    }

    #[test]
    fn synthetic_exports_keep_identity_and_satisfy_imports() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let shared = Object::new(token.ctx()).unwrap();

            let (namespace, promise) = QuickJs::evaluate(
                token,
                QuickJs::synthetic(
                    token,
                    "shared",
                    vec![("value".to_string(), shared.clone().into_value())],
                )
                .unwrap(),
            )
            .unwrap();

            let (importer, _) = QuickJs::evaluate(
                token,
                QuickJs::declare(
                    token,
                    "importer",
                    r#"
                    import { value } from "shared";

                    export { value };
                    "#,
                )
                .unwrap(),
            )
            .unwrap();

            assert_eq!(promise.as_promise().unwrap().state(), PromiseState::Resolved);
            assert_eq!(
                namespace
                    .as_object()
                    .unwrap()
                    .get::<_, Object>("value")
                    .unwrap(),
                shared,
            );
            assert_eq!(
                importer
                    .as_object()
                    .unwrap()
                    .get::<_, Object>("value")
                    .unwrap(),
                shared,
            );
        });
    }

    #[test]
    fn synthetic_rejects_invalid_names_without_a_record() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(matches!(
                QuickJs::synthetic(
                    token,
                    "twice",
                    vec![
                        ("a".to_string(), QuickJs::number(token, 1.0)),
                        ("a".to_string(), QuickJs::number(token, 2.0)),
                    ],
                ),
                Err(Error::Guest(exception)) if exception.name() == "SyntaxError",
            ));
            assert!(
                QuickJs::synthetic(
                    token,
                    "nul",
                    vec![("a\0b".to_string(), QuickJs::number(token, 1.0))],
                )
                .is_err_and(|error| error.is_engine()),
            );

            QuickJs::synthetic(
                token,
                "twice",
                vec![("a".to_string(), QuickJs::number(token, 3.0))],
            )
            .unwrap();

            let (namespace, _) = QuickJs::evaluate(
                token,
                QuickJs::declare(token, "reader", r#"export { a } from "twice";"#).unwrap(),
            )
            .unwrap();

            assert_eq!(
                namespace
                    .as_object()
                    .unwrap()
                    .get::<_, f64>("a")
                    .unwrap(),
                3.0,
            );
        });
    }

    #[test]
    fn dispatcher_loads_nested_and_dynamic_imports() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::install(&engine, Rc::new(Sources)).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            let (namespace, _) = QuickJs::evaluate(
                token,
                QuickJs::declare(
                    token,
                    "main",
                    r#"
                    import { two } from "double";
                    import { one } from "numbers";

                    export const three = one + two;
                    "#,
                )
                .unwrap(),
            )
            .unwrap();

            let dynamic = QuickJs::eval(
                token,
                r#"import("double").then((module) => module.two)"#,
                "dynamic.js",
            )
            .unwrap();

            while token.ctx().execute_pending_job() {}

            assert_eq!(
                namespace
                    .as_object()
                    .unwrap()
                    .get::<_, f64>("three")
                    .unwrap(),
                3.0,
            );
            assert_eq!(
                dynamic
                    .as_promise()
                    .unwrap()
                    .result::<f64>()
                    .unwrap()
                    .unwrap(),
                2.0,
            );
        });
    }

    #[test]
    fn dispatcher_failures_fail_the_import() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::install(&engine, Rc::new(Sources)).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            assert!(matches!(
                QuickJs::declare(token, "missing", r#"import "nowhere";"#),
                Err(Error::Guest(exception)) if exception.name() == "TypeError",
            ));
            assert!(matches!(
                QuickJs::declare(token, "renaming", r#"import "renamed";"#),
                Err(Error::Guest(exception)) if exception.name() == "TypeError",
            ));
            assert!(matches!(
                QuickJs::declare(
                    token,
                    "attributed",
                    r#"import "numbers" with { type: "json" };"#,
                ),
                Err(Error::Guest(exception)) if exception.name() == "SyntaxError",
            ));
        });
    }

    #[test]
    fn install_inside_entry_is_unsupported() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        assert!(
            QuickJs::enter(&engine, &context, |_| QuickJs::install(&engine, Rc::new(Sources)))
                .is_err_and(|error| error.is_unsupported()),
        );
    }
}
