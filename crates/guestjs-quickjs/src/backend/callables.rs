use guestjs_core::{
    backend::{
        callables::{
            BackendCallables,
            RawBody,
            RawCall,
        },
        Tok,
        Val,
    },
    errors::Error,
};
use rquickjs::{
    function::{
        Params,
        RustFunction,
    },
    Class,
    Function,
    Value,
};

use crate::{
    backend::QuickJs,
    context::Token,
    error::{
        Capture,
        Raise,
    },
};

impl BackendCallables for QuickJs {
    fn function<'js>(
        token: Tok<'js, Self>,
        name: &str,
        length: u32,
        body: RawBody<Self>,
    ) -> Result<Val<'js, Self>, Error> {
        Class::instance(
            token.ctx(),
            RustFunction(Box::new(move |params: Params<'_, 'js>| {
                let token = Token::from_ctx(params.ctx());

                body(RawCall {
                    token,
                    this: match params.is_constructor() {
                        true => Value::new_undefined(params.ctx().clone()),
                        false => params.this(),
                    },
                    args: (0..params.len())
                        .filter_map(|index| params.arg(index))
                        .collect(),
                    new_target: params
                        .is_constructor()
                        .then(|| params.this()),
                })
                .map_err(|error| error.raise(token))
            })),
        )
        .and_then(|function| Function::from_value(function.into_value()))
        .and_then(|function| function.with_length(length as usize))
        .and_then(|function| function.with_name(name))
        .map(Function::into_value)
        .capture(token)
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use guestjs_core::{
        backend::{
            callables::{
                BackendCallables,
                RawCall,
            },
            exceptions::BackendExceptions,
            values::BackendValues,
            Backend,
        },
        errors::Error,
    };

    use crate::{
        backend::QuickJs,
        engine::Config,
    };

    #[test]
    fn function_receives_this_and_arguments() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::set(
                token,
                &QuickJs::globals(token),
                &QuickJs::string(token, "describe").unwrap(),
                QuickJs::function(
                    token,
                    "describe",
                    2,
                    Rc::new(|call: RawCall<'_, QuickJs>| {
                        QuickJs::array(
                            call.token,
                            vec![
                                call.this,
                                QuickJs::number(call.token, call.args.len() as f64),
                            ],
                        )
                    }),
                )
                .unwrap(),
            )
            .unwrap();

            assert!(token
                .ctx()
                .eval::<bool, _>(
                    r#"
                    const receiver = { describe };
                    const [self, count] = receiver.describe(1, 2, 3);
                    self === receiver
                        && count === 3
                        && describe.name === "describe"
                        && describe.length === 2
                        && Object.getOwnPropertyNames(describe).join() === "length,name"
                        && Object.getPrototypeOf(describe) === Function.prototype
                    "#,
                )
                .unwrap());
            assert!(token
                .ctx()
                .eval::<bool, _>(
                    r#"
                    try {
                        new describe();
                        false;
                    } catch (error) {
                        error instanceof TypeError;
                    }
                    "#,
                )
                .unwrap());
        });
    }

    #[test]
    fn function_errors_are_raised() {
        let engine = QuickJs::engine(Config::default()).unwrap();
        let context = QuickJs::new_context(&engine).unwrap();

        QuickJs::enter(&engine, &context, |token| {
            QuickJs::set(
                token,
                &QuickJs::globals(token),
                &QuickJs::string(token, "fail").unwrap(),
                QuickJs::function(
                    token,
                    "fail",
                    0,
                    Rc::new(|_: RawCall<'_, QuickJs>| {
                        Err(Error::conversion("number", "string"))
                    }),
                )
                .unwrap(),
            )
            .unwrap();
            QuickJs::set(
                token,
                &QuickJs::globals(token),
                &QuickJs::string(token, "rethrow").unwrap(),
                QuickJs::function(
                    token,
                    "rethrow",
                    1,
                    Rc::new(|call: RawCall<'_, QuickJs>| {
                        Err(QuickJs::throw(call.token, call.args[0].clone()))
                    }),
                )
                .unwrap(),
            )
            .unwrap();

            assert!(token
                .ctx()
                .eval::<bool, _>(
                    r#"
                    try {
                        fail();
                        false;
                    } catch (error) {
                        error instanceof TypeError && error.message === "expected number, found string";
                    }
                    "#,
                )
                .unwrap());
            assert!(token
                .ctx()
                .eval::<bool, _>(
                    r#"
                    const original = {};
                    try {
                        rethrow(original);
                        false;
                    } catch (error) {
                        error === original;
                    }
                    "#,
                )
                .unwrap());
        });
    }
}
