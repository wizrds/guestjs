use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path, ReceiverKind, Signature};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Receiver {
    None,
    Shared,
    Exclusive,
}

impl Receiver {
    pub fn new(signature: &Signature) -> syn::Result<Self> {
        match signature.receiver() {
            None => Ok(Self::None),
            Some(receiver) => match receiver.kind {
                ReceiverKind::Reference(_, _, Some(_)) => Ok(Self::Exclusive),
                ReceiverKind::Reference(_, _, None) => Ok(Self::Shared),
                _ => Err(syn::Error::new_spanned(
                    receiver,
                    "a host receiver requires &self or &mut self",
                )),
            },
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn invoke(
        &self,
        ident: &Ident,
        turbofish: &TokenStream,
        context: &TokenStream,
        args: &TokenStream,
        arguments: &[TokenStream],
    ) -> TokenStream {
        match self {
            Self::None => {
                quote!(Self::#ident #turbofish (#(#arguments),*))
            }
            Self::Shared => {
                quote! {
                    (&*#args.this_ref::<Self>(#context.enter())?)
                        .#ident #turbofish (#(#arguments),*)
                }
            }
            Self::Exclusive => {
                quote! {
                    (&mut *#args.this_mut::<Self>(#context.enter())?)
                        .#ident #turbofish (#(#arguments),*)
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModuleReceiver {
    None,
    Shared,
}

impl ModuleReceiver {
    pub fn new(signature: &Signature) -> syn::Result<Self> {
        match signature.receiver() {
            None => Ok(Self::None),
            Some(receiver) => match receiver.kind {
                ReceiverKind::Reference(_, _, None) => Ok(Self::Shared),
                _ => Err(syn::Error::new_spanned(receiver, "a module receiver requires &self")),
            },
        }
    }

    pub fn requires_state(&self) -> bool {
        matches!(self, Self::Shared)
    }

    pub fn invoke(
        &self,
        krate: &Path,
        ident: &Ident,
        turbofish: &TokenStream,
        context: &TokenStream,
        arguments: &[TokenStream],
    ) -> TokenStream {
        match self {
            Self::None => {
                quote!(Self::#ident #turbofish (#(#arguments),*))
            }
            Self::Shared => {
                quote! {
                    #context
                        .resolve::<#krate::host::state::ModuleState<Self>>()?
                        .#ident #turbofish (#(#arguments),*)
                }
            }
        }
    }

    pub fn preparation(&self, krate: &Path, context: &TokenStream, state: &Ident) -> TokenStream {
        match self {
            Self::None => TokenStream::new(),
            Self::Shared => quote! {
                let #state = #context
                    .resolve::<#krate::host::state::ModuleState<Self>>()?;
            },
        }
    }

    pub fn invoke_prepared(
        &self,
        ident: &Ident,
        turbofish: &TokenStream,
        state: &Ident,
        arguments: &[TokenStream],
    ) -> TokenStream {
        match self {
            Self::None => {
                quote!(Self::#ident #turbofish (#(#arguments),*))
            }
            Self::Shared => {
                quote!(#state.#ident #turbofish (#(#arguments),*))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{Expr, parse_quote, parse2};

    use crate::host::receiver::{ModuleReceiver, Receiver};

    #[test]
    fn shared_payload_receiver_holds_a_checked_borrow() {
        assert_eq!(
            parse2::<Expr>(Receiver::Shared.invoke(
                &parse_quote!(read),
                &quote!(),
                &quote!(context),
                &quote!(args),
                &[quote!(1)],
            ))
            .unwrap(),
            parse_quote!((&*args.this_ref::<Self>(context.enter())?).read(1)),
        );
    }

    #[test]
    fn exclusive_payload_receiver_dereferences_the_guard() {
        assert_eq!(
            parse2::<Expr>(Receiver::Exclusive.invoke(
                &parse_quote!(write),
                &quote!(),
                &quote!(context),
                &quote!(args),
                &[quote!(1)],
            ))
            .unwrap(),
            parse_quote!((&mut *args.this_mut::<Self>(context.enter())?).write(1)),
        );
    }

    #[test]
    fn shared_module_receiver_resolves_captured_state() {
        assert_eq!(
            parse2::<Expr>(ModuleReceiver::Shared.invoke(
                &parse_quote!(crate),
                &parse_quote!(read),
                &quote!(),
                &quote!(context),
                &[],
            ))
            .unwrap(),
            parse_quote!(
                context
                    .resolve::<crate::host::state::ModuleState<Self>>()?
                    .read()
            ),
        );
    }

    #[test]
    fn rejects_owned_and_typed_receivers() {
        for signature in [
            parse_quote!(fn consume(self) -> Result<(), Error>),
            parse_quote!(fn consume(self: Box<Self>) -> Result<(), Error>),
        ] {
            assert!(Receiver::new(&signature).is_err());
            assert!(ModuleReceiver::new(&signature).is_err());
        }
    }

    #[test]
    fn rejects_exclusive_module_receivers() {
        assert!(
            ModuleReceiver::new(&parse_quote!(
                fn update(&mut self) -> Result<(), Error>
            ))
            .is_err()
        );
    }
}
