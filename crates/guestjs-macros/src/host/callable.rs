use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    FnArg, GenericParam, Ident, Path, ReturnType, Safety, Signature, Type, spanned::Spanned,
};

use crate::{
    backend::BackendParameter,
    diagnostics::MacroError,
    host::{
        parameter::{Parameter, ParameterSyntax},
        receiver::{ModuleReceiver, Receiver},
    },
};

struct AsyncArguments {
    names: Vec<Ident>,
    values: Vec<TokenStream>,
}

impl AsyncArguments {
    fn new(parameters: &[Parameter], context: &TokenStream, args: &TokenStream) -> Self {
        Self {
            names: parameters
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    Ident::new(&format!("__guestjs_argument_{index}"), Span::mixed_site())
                })
                .collect(),
            values: parameters
                .iter()
                .map(|parameter| parameter.expression(context, args))
                .collect(),
        }
    }

    fn preparation(&self) -> TokenStream {
        if self.names.is_empty() {
            return TokenStream::new();
        }

        let names = &self.names;
        let values = &self.values;

        quote!(let (#(#names,)*) = (#(#values,)*);)
    }

    fn arguments(&self) -> Vec<TokenStream> {
        self.names
            .iter()
            .map(|name| quote!(#name))
            .collect()
    }
}

pub struct Callable {
    ident: Ident,
    receiver: Receiver,
    parameters: Vec<Parameter>,
    asynchronous: bool,
    span: Span,
}

impl Callable {
    pub fn new(signature: &mut Signature) -> Result<Self, MacroError> {
        if matches!(signature.safety, Safety::Unsafe(_))
            || signature.abi.is_some()
            || signature.variadic.is_some()
        {
            return Err(syn::Error::new_spanned(
                &*signature,
                "unsafe, foreign-ABI, and variadic host callables are unsupported",
            )
            .into());
        }

        if signature
            .generics
            .params
            .iter()
            .any(|parameter| !matches!(parameter, GenericParam::Lifetime(_)))
        {
            return Err(syn::Error::new_spanned(
                &signature.generics,
                "host callables cannot declare type or const parameters",
            )
            .into());
        }

        if matches!(signature.output, ReturnType::Default) {
            return Err(syn::Error::new_spanned(
                &*signature,
                "a host callable requires a written return type",
            )
            .into());
        }

        let receiver = Receiver::new(signature)?;
        let mut parameters = Vec::new();
        let mut rest = false;

        for input in &mut signature.inputs {
            let FnArg::Typed(input) = input else {
                continue;
            };

            let parameter = ParameterSyntax::parse(input)?;

            if rest && parameter.consumes_argument() {
                return Err(syn::Error::new(
                    parameter.ident().span(),
                    "rest must be the last positional parameter",
                )
                .into());
            }

            rest |= parameter.is_rest();
            parameters.push(parameter);
        }

        Ok(Self {
            ident: signature.ident.clone(),
            receiver,
            parameters,
            asynchronous: signature.asyncness.is_some(),
            span: signature.span(),
        })
    }

    fn error(&self, message: &str) -> syn::Error {
        syn::Error::new(self.span, message)
    }

    fn arguments(&self, context: &TokenStream, args: &TokenStream) -> Vec<TokenStream> {
        self.parameters
            .iter()
            .map(|parameter| parameter.expression(context, args))
            .collect()
    }

    pub fn ident(&self) -> &Ident {
        &self.ident
    }

    pub fn is_async(&self) -> bool {
        self.asynchronous
    }

    pub fn has_receiver(&self) -> bool {
        !self.receiver.is_none()
    }

    pub fn state_types(&self) -> Vec<Type> {
        let mut types = Vec::new();

        for ty in self
            .parameters
            .iter()
            .filter_map(Parameter::state_type)
        {
            if !types.contains(ty) {
                types.push(ty.clone());
            }
        }

        types
    }

    pub fn validate_async(&self, module: bool) -> syn::Result<()> {
        if self.asynchronous
            && ((!module && self.has_receiver())
                || self
                    .parameters
                    .iter()
                    .any(Parameter::borrows_context_or_argument))
        {
            return Err(self.error(
                "async host functions require owned arguments and no borrowed class receiver, context, entry, or this",
            ));
        }

        Ok(())
    }

    pub fn validate_constructor(&self) -> syn::Result<()> {
        if self.has_receiver() || self.asynchronous {
            return Err(self.error("a constructor must be synchronous and have no receiver"));
        }

        Ok(())
    }

    pub fn validate_instance(&self) -> syn::Result<()> {
        if !self.has_receiver()
            && !self
                .parameters
                .iter()
                .any(Parameter::is_this)
        {
            return Err(
                self.error("an instance member requires a payload receiver or explicit this")
            );
        }

        Ok(())
    }

    pub fn validate_getter(&self) -> syn::Result<()> {
        self.validate_instance()?;

        if self.asynchronous
            || self
                .parameters
                .iter()
                .any(Parameter::consumes_argument)
        {
            return Err(self.error("a getter must be synchronous and consume no positional values"));
        }

        Ok(())
    }

    pub fn validate_setter(&self) -> syn::Result<()> {
        self.validate_instance()?;

        if self.asynchronous
            || self
                .parameters
                .iter()
                .filter(|parameter| parameter.consumes_argument())
                .count()
                != 1
            || self
                .parameters
                .iter()
                .filter(|parameter| parameter.is_value())
                .count()
                != 1
        {
            return Err(
                self.error("a setter must be synchronous and take one ordinary positional value")
            );
        }

        Ok(())
    }

    pub fn validate_static(&self) -> syn::Result<()> {
        if self.has_receiver() {
            return Err(self.error("a static member cannot have a payload receiver"));
        }

        Ok(())
    }

    pub fn validate_namespace_factory(&self) -> syn::Result<()> {
        if self.has_receiver() || self.asynchronous || !self.parameters.is_empty() {
            return Err(self.error("a namespace factory must be synchronous and take no arguments"));
        }

        Ok(())
    }

    pub fn validate_factory(&self) -> syn::Result<()> {
        if self.has_receiver()
            || self.asynchronous
            || self
                .parameters
                .iter()
                .any(|parameter| !parameter.is_enter())
        {
            return Err(self.error(
                "a state factory must be synchronous and accept only injected entry parameters",
            ));
        }

        Ok(())
    }

    pub fn associated(&self, backend: &BackendParameter) -> TokenStream {
        let ident = &self.ident;
        let turbofish = backend.turbofish();

        quote!(Self::#ident #turbofish ())
    }

    pub fn factory(&self, backend: &BackendParameter) -> TokenStream {
        let ident = &self.ident;
        let turbofish = backend.turbofish();
        let enter = Ident::new("__guestjs_enter", Span::mixed_site());
        let arguments = self
            .parameters
            .iter()
            .map(|_| quote!(#enter));

        quote!(|#enter| Self::#ident #turbofish (#(#arguments),*))
    }

    pub fn class_callback(&self, backend: &BackendParameter) -> TokenStream {
        let context = Ident::new("__guestjs_context", Span::mixed_site());
        let args = Ident::new("__guestjs_args", Span::mixed_site());

        if self.asynchronous {
            let prepared = AsyncArguments::new(&self.parameters, &quote!(#context), &quote!(#args));
            let preparation = prepared.preparation();
            let invocation = self.receiver.invoke(
                &self.ident,
                &backend.turbofish(),
                &quote!(#context),
                &quote!(#args),
                &prepared.arguments(),
            );

            quote! {
                |#context, #args| {
                    #preparation

                    ::core::result::Result::Ok(async move {
                        #invocation
                            .await
                            .map_err(::core::convert::Into::into)
                    })
                }
            }
        } else {
            let invocation = self.receiver.invoke(
                &self.ident,
                &backend.turbofish(),
                &quote!(#context),
                &quote!(#args),
                &self.arguments(&quote!(#context), &quote!(#args)),
            );

            quote! {
                |#context, #args| {
                    #invocation.map_err(::core::convert::Into::into)
                }
            }
        }
    }

    pub fn module_callback(
        &self,
        krate: &Path,
        backend: &BackendParameter,
        receiver: &ModuleReceiver,
    ) -> TokenStream {
        let context = Ident::new("__guestjs_context", Span::mixed_site());
        let args = Ident::new("__guestjs_args", Span::mixed_site());

        if self.asynchronous {
            let state = Ident::new("__guestjs_state", Span::mixed_site());
            let prepared = AsyncArguments::new(&self.parameters, &quote!(#context), &quote!(#args));
            let preparation = prepared.preparation();
            let state_preparation = receiver.preparation(krate, &quote!(#context), &state);
            let invocation = receiver.invoke_prepared(
                &self.ident,
                &backend.turbofish(),
                &state,
                &prepared.arguments(),
            );

            quote! {
                |#context, #args| {
                    #preparation
                    #state_preparation

                    ::core::result::Result::Ok(async move {
                        #invocation
                            .await
                            .map_err(::core::convert::Into::into)
                    })
                }
            }
        } else {
            let invocation = receiver.invoke(
                krate,
                &self.ident,
                &backend.turbofish(),
                &quote!(#context),
                &self.arguments(&quote!(#context), &quote!(#args)),
            );

            quote! {
                |#context, #args| {
                    #invocation.map_err(::core::convert::Into::into)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;
    use syn::{Expr, Signature, parse_quote, parse2};

    use crate::{
        backend::BackendParameter,
        host::{callable::Callable, receiver::ModuleReceiver},
    };

    struct CallableFixture;

    impl CallableFixture {
        fn parse(mut signature: Signature) -> Callable {
            Callable::new(&mut signature).unwrap_or_else(|error| panic!("{}", error.into_tokens()))
        }

        fn backend() -> BackendParameter {
            BackendParameter::resolve(None, &Default::default(), &[]).unwrap()
        }

        fn expression(tokens: TokenStream) -> Expr {
            parse2(tokens).unwrap()
        }
    }

    #[test]
    fn synchronous_borrows_are_inline() {
        assert_eq!(
            CallableFixture::expression(
                CallableFixture::parse(parse_quote!(
                    fn read(&self, #[guestjs(borrow)] point: &Point)
                        -> Result<f64, Error>
                ))
                .class_callback(&CallableFixture::backend()),
            ),
            parse_quote!(|__guestjs_context, __guestjs_args| {
                (&*__guestjs_args.this_ref::<Self>(__guestjs_context.enter())?)
                    .read(&*__guestjs_args.next_ref::<Point>(__guestjs_context.enter())?)
                    .map_err(::core::convert::Into::into)
            }),
        );
    }

    #[test]
    fn allows_injection_after_rest_but_not_positionals() {
        assert!(
            Callable::new(&mut parse_quote!(
                fn receive(
                    #[guestjs(rest)] values: Vec<String>,
                    #[guestjs(enter)] enter: &Enter<'_, B>,
                ) -> Result<(), Error>
            ))
            .is_ok()
        );
        assert!(
            Callable::new(&mut parse_quote!(
                fn receive(
                    #[guestjs(rest)] values: Vec<String>,
                    value: String,
                ) -> Result<(), Error>
            ))
            .is_err()
        );
        assert!(
            Callable::new(&mut parse_quote!(
                fn receive(
                    #[guestjs(rest)] first: Vec<String>,
                    #[guestjs(rest)] second: Vec<String>,
                ) -> Result<(), Error>
            ))
            .is_err()
        );
    }

    #[test]
    fn rejects_unsupported_source_shapes() {
        for mut signature in [
            parse_quote!(unsafe fn read() -> Result<(), Error>),
            parse_quote!(extern "C" fn read() -> Result<(), Error>),
            parse_quote!(fn read<T>() -> Result<(), Error>),
            parse_quote!(fn read<const N: usize>() -> Result<(), Error>),
            parse_quote!(fn read()),
        ] {
            assert!(Callable::new(&mut signature).is_err());
        }
    }

    #[test]
    fn preserves_lifetime_generics_and_unrelated_parameter_attributes() {
        let mut signature = parse_quote!(
            fn read<'a>(
                #[allow(unused_mut)]
                #[guestjs(borrow)]
                mut point: &'a Point,
            ) -> Result<f64, Error>
        );

        assert!(Callable::new(&mut signature).is_ok());
        assert_eq!(
            signature,
            parse_quote!(
                fn read<'a>(
                    #[allow(unused_mut)]
                    mut point: &'a Point,
                ) -> Result<f64, Error>
            ),
        );
    }

    #[test]
    fn source_async_functions_cannot_capture_callback_borrows() {
        for signature in [
            parse_quote!(
                async fn read(#[guestjs(enter)] enter: &Enter<'_, B>)
                    -> Result<(), Error>
            ),
            parse_quote!(
                async fn read(#[guestjs(borrow)] point: &Point)
                    -> Result<(), Error>
            ),
            parse_quote!(
                async fn read(#[guestjs(this)] this: &Object<B>)
                    -> Result<(), Error>
            ),
        ] {
            assert!(
                CallableFixture::parse(signature)
                    .validate_async(true)
                    .is_err()
            );
        }
    }

    #[test]
    fn synchronous_future_factories_can_prepare_from_borrows() {
        assert!(
            CallableFixture::parse(parse_quote!(
                fn prepare(&mut self, #[guestjs(borrow)] point: &Point)
                    -> Result<impl Future<Output = Result<(), Error>>, Error>
            ))
            .validate_async(false)
            .is_ok()
        );
    }

    #[test]
    fn async_module_state_and_arguments_are_prepared_before_suspension() {
        assert_eq!(
            CallableFixture::expression(
                CallableFixture::parse(parse_quote!(
                    async fn send(&self, request: String)
                        -> Result<String, Error>
                ))
                .module_callback(
                    &parse_quote!(crate),
                    &CallableFixture::backend(),
                    &ModuleReceiver::Shared,
                ),
            ),
            parse_quote!(|__guestjs_context, __guestjs_args| {
                let (__guestjs_argument_0,) =
                    (__guestjs_args.next::<String>(__guestjs_context.enter())?,);
                let __guestjs_state =
                    __guestjs_context.resolve::<crate::host::state::ModuleState<Self>>()?;

                ::core::result::Result::Ok(async move {
                    __guestjs_state
                        .send(__guestjs_argument_0)
                        .await
                        .map_err(::core::convert::Into::into)
                })
            }),
        );
    }

    #[test]
    fn empty_async_arguments_emit_no_preparation_statement() {
        assert_eq!(
            CallableFixture::expression(
                CallableFixture::parse(parse_quote!(
                    async fn run() -> Result<(), Error>
                ))
                .module_callback(
                    &parse_quote!(crate),
                    &CallableFixture::backend(),
                    &ModuleReceiver::None,
                ),
            ),
            parse_quote!(|__guestjs_context, __guestjs_args| {
                ::core::result::Result::Ok(async move {
                    Self::run()
                        .await
                        .map_err(::core::convert::Into::into)
                })
            }),
        );
    }

    #[test]
    fn requirements_are_deduplicated_in_first_occurrence_order() {
        assert_eq!(
            CallableFixture::parse(parse_quote!(
                fn run(
                    #[guestjs(state)] first: ModuleState<First>,
                    #[guestjs(state)] again: ModuleState<First>,
                    #[guestjs(state)] second: ModuleState<Second>,
                ) -> Result<(), Error>
            ))
            .state_types(),
            vec![
                parse_quote!(ModuleState<First>),
                parse_quote!(ModuleState<Second>),
            ],
        );
    }
}
