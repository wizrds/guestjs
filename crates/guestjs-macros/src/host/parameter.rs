use darling::{FromMeta, util::Flag};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Error as SyntaxError, GenericArgument, Ident, Pat, PatType, PathArguments,
    Result as SyntaxResult, Type,
};

use crate::{attributes::HelperAttributes, diagnostics::MacroError};

struct TypeShape;

impl TypeShape {
    fn inner<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
        let Type::Path(path) = ty else {
            return None;
        };

        if path.qself.is_some() {
            return None;
        }

        let segment = path.path.segments.last()?;

        if segment.ident != name {
            return None;
        }

        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return None;
        };

        if arguments.args.len() != 1 {
            return None;
        }

        match arguments.args.first()? {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        }
    }

    fn reference(ty: &Type, mutable: bool) -> SyntaxResult<&Type> {
        match ty {
            Type::Reference(reference) if reference.mutability.is_some() == mutable => {
                Ok(reference.elem.as_ref())
            }
            _ => Err(SyntaxError::new_spanned(
                ty,
                match mutable {
                    true => "expected &mut T",
                    false => "expected &T",
                },
            )),
        }
    }
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ParameterOptions {
    borrow: Flag,
    borrow_mut: Flag,
    rest: Flag,
    enter: Flag,
    this: Flag,
    context: Flag,
    state: Flag,
}

impl ParameterOptions {
    fn resolve(&self, ty: &Type) -> SyntaxResult<ParameterRole> {
        if [
            self.borrow.is_present(),
            self.borrow_mut.is_present(),
            self.rest.is_present(),
            self.enter.is_present(),
            self.this.is_present(),
            self.context.is_present(),
            self.state.is_present(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
            > 1
        {
            return Err(SyntaxError::new_spanned(
                ty,
                "a parameter may declare only one guestjs role",
            ));
        }

        if self.borrow.is_present() {
            return TypeShape::reference(ty, false)
                .cloned()
                .map(ParameterRole::Borrow);
        }

        if self.borrow_mut.is_present() {
            return TypeShape::reference(ty, true)
                .cloned()
                .map(ParameterRole::BorrowMut);
        }

        if self.rest.is_present() {
            return TypeShape::inner(ty, "Vec")
                .cloned()
                .map(ParameterRole::Rest)
                .ok_or_else(|| SyntaxError::new_spanned(ty, "rest requires Vec<T>"));
        }

        if self.enter.is_present() || self.context.is_present() {
            TypeShape::reference(ty, false)?;

            return Ok(match self.enter.is_present() {
                true => ParameterRole::Enter,
                false => ParameterRole::Context,
            });
        }

        if self.state.is_present() {
            if TypeShape::inner(ty, "ModuleState").is_none() {
                return Err(SyntaxError::new_spanned(ty, "state requires ModuleState<S>"));
            }

            return Ok(ParameterRole::State(ty.clone()));
        }

        if self.this.is_present() {
            return match ty {
                Type::Reference(_) => TypeShape::reference(ty, false)
                    .cloned()
                    .map(ParameterRole::ThisRef),
                _ => Ok(ParameterRole::This(ty.clone())),
            };
        }

        Ok(ParameterRole::Value(ty.clone()))
    }
}

pub enum ParameterRole {
    Value(Type),
    Borrow(Type),
    BorrowMut(Type),
    Rest(Type),
    Enter,
    This(Type),
    ThisRef(Type),
    Context,
    State(Type),
}

impl ParameterRole {
    pub fn expression(&self, context: &TokenStream, args: &TokenStream) -> TokenStream {
        match self {
            Self::Value(descriptor) => {
                quote!(#args.next::<#descriptor>(#context.enter())?)
            }
            Self::Borrow(payload) => {
                quote!(&*#args.next_ref::<#payload>(#context.enter())?)
            }
            Self::BorrowMut(payload) => {
                quote!(&mut *#args.next_mut::<#payload>(#context.enter())?)
            }
            Self::Rest(descriptor) => {
                quote!(#args.rest::<#descriptor>(#context.enter())?)
            }
            Self::Enter => quote!(#context.enter()),
            Self::This(descriptor) => {
                quote!(#args.this::<#descriptor>(#context.enter())?)
            }
            Self::ThisRef(descriptor) => {
                quote!(&#args.this::<#descriptor>(#context.enter())?)
            }
            Self::Context => quote!(#context),
            Self::State(state) => quote!(#context.resolve::<#state>()?),
        }
    }

    pub fn is_this(&self) -> bool {
        matches!(self, Self::This(_) | Self::ThisRef(_))
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    pub fn is_enter(&self) -> bool {
        matches!(self, Self::Enter)
    }

    pub fn state_type(&self) -> Option<&Type> {
        match self {
            Self::State(ty) => Some(ty),
            _ => None,
        }
    }

    pub fn consumes_argument(&self) -> bool {
        matches!(self, Self::Value(_) | Self::Borrow(_) | Self::BorrowMut(_) | Self::Rest(_),)
    }

    pub fn is_rest(&self) -> bool {
        matches!(self, Self::Rest(_))
    }

    pub fn borrows_context_or_argument(&self) -> bool {
        matches!(
            self,
            Self::Borrow(_) | Self::BorrowMut(_) | Self::Enter | Self::Context | Self::ThisRef(_),
        )
    }
}

pub struct Parameter {
    ident: Ident,
    role: ParameterRole,
}

impl Parameter {
    pub fn new(ident: Ident, role: ParameterRole) -> Self {
        Self { ident, role }
    }

    pub fn ident(&self) -> &Ident {
        &self.ident
    }

    pub fn expression(&self, context: &TokenStream, args: &TokenStream) -> TokenStream {
        self.role.expression(context, args)
    }

    pub fn state_type(&self) -> Option<&syn::Type> {
        self.role.state_type()
    }

    pub fn consumes_argument(&self) -> bool {
        self.role.consumes_argument()
    }

    pub fn is_rest(&self) -> bool {
        self.role.is_rest()
    }

    pub fn borrows_context_or_argument(&self) -> bool {
        self.role.borrows_context_or_argument()
    }

    pub fn is_this(&self) -> bool {
        self.role.is_this()
    }

    pub fn is_value(&self) -> bool {
        self.role.is_value()
    }

    pub fn is_enter(&self) -> bool {
        self.role.is_enter()
    }
}

pub struct ParameterSyntax;

impl ParameterSyntax {
    pub fn parse(argument: &mut PatType) -> Result<Parameter, MacroError> {
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(SyntaxError::new_spanned(
                argument.pat.as_ref(),
                "a host parameter requires an identifier pattern",
            )
            .into());
        };

        if pattern.by_ref.is_some() || pattern.subpat.is_some() {
            return Err(SyntaxError::new_spanned(
                pattern,
                "ref and @ parameter patterns are unsupported",
            )
            .into());
        }

        Ok(Parameter::new(
            pattern.ident.clone(),
            ParameterOptions::from_list(&HelperAttributes::take(&mut argument.attrs)?)?
                .resolve(argument.ty.as_ref())?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{Expr, FnArg, PatType, parse_quote, parse2};

    use crate::host::parameter::{ParameterRole, ParameterSyntax};

    struct ParameterFixture;

    impl ParameterFixture {
        fn typed(argument: FnArg) -> PatType {
            match argument {
                FnArg::Typed(parameter) => parameter,
                FnArg::Receiver(_) => panic!("expected a typed parameter"),
            }
        }
    }

    #[test]
    fn emits_every_parameter_role() {
        for (role, expected) in [
            (
                ParameterRole::Value(parse_quote!(String)),
                parse_quote!(args.next::<String>(context.enter())?),
            ),
            (
                ParameterRole::Borrow(parse_quote!(Point)),
                parse_quote!(&*args.next_ref::<Point>(context.enter())?),
            ),
            (
                ParameterRole::BorrowMut(parse_quote!(Point)),
                parse_quote!(&mut *args.next_mut::<Point>(context.enter())?),
            ),
            (
                ParameterRole::Rest(parse_quote!(Point)),
                parse_quote!(args.rest::<Point>(context.enter())?),
            ),
            (ParameterRole::Enter, parse_quote!(context.enter())),
            (
                ParameterRole::This(parse_quote!(Object<B>)),
                parse_quote!(args.this::<Object<B>>(context.enter())?),
            ),
            (
                ParameterRole::ThisRef(parse_quote!(Object<B>)),
                parse_quote!(&args.this::<Object<B>>(context.enter())?),
            ),
            (ParameterRole::Context, parse_quote!(context)),
            (
                ParameterRole::State(parse_quote!(ModuleState<Settings>)),
                parse_quote!(context.resolve::<ModuleState<Settings>>()?),
            ),
        ] {
            assert_eq!(
                parse2::<Expr>(role.expression(&quote!(context), &quote!(args),)).unwrap(),
                expected,
            );
        }
    }

    #[test]
    fn retains_the_complete_state_type() {
        assert_eq!(
            ParameterSyntax::parse(&mut ParameterFixture::typed(parse_quote!(
                #[guestjs(state)] state: ModuleState<Settings>
            )))
            .unwrap_or_else(|error| panic!("{}", error.into_tokens()))
            .state_type(),
            Some(&parse_quote!(ModuleState<Settings>)),
        );
    }

    #[test]
    fn rejects_conflicting_roles() {
        assert!(
            ParameterSyntax::parse(&mut ParameterFixture::typed(parse_quote!(
                #[guestjs(borrow, rest)] values: &Vec<Point>
            )))
            .is_err()
        );
    }

    #[test]
    fn rejects_invalid_annotated_types() {
        for mut parameter in [
            parse_quote!(#[guestjs(borrow)] point: Point),
            parse_quote!(#[guestjs(borrow)] point: &mut Point),
            parse_quote!(#[guestjs(borrow_mut)] point: &Point),
            parse_quote!(#[guestjs(rest)] values: String),
            parse_quote!(#[guestjs(rest)] values: Vec<Point, Other>),
            parse_quote!(#[guestjs(state)] state: Settings),
            parse_quote!(#[guestjs(state)] state: StateAlias),
            parse_quote!(#[guestjs(enter)] enter: Enter<'_, B>),
            parse_quote!(#[guestjs(context)] context: &mut CallContext<'_, '_, B>),
            parse_quote!(#[guestjs(this)] this: &mut Object<B>),
        ]
        .map(ParameterFixture::typed)
        {
            assert!(ParameterSyntax::parse(&mut parameter).is_err());
        }
    }

    #[test]
    fn rejects_non_identifier_patterns() {
        for mut parameter in [
            parse_quote!((first, second): (u8, u8)),
            parse_quote!(ref value: String),
            parse_quote!(value @ _: String),
        ]
        .map(ParameterFixture::typed)
        {
            assert!(ParameterSyntax::parse(&mut parameter).is_err());
        }
    }

    #[test]
    fn preserves_mutable_bindings_and_other_attributes() {
        let mut parameter = ParameterFixture::typed(parse_quote!(
            #[allow(unused_mut)]
            #[guestjs(borrow)]
            mut point: &Point
        ));

        assert!(ParameterSyntax::parse(&mut parameter).is_ok());
        assert_eq!(
            parameter,
            ParameterFixture::typed(parse_quote!(
                #[allow(unused_mut)] mut point: &Point
            )),
        );
    }

    #[test]
    fn injected_roles_do_not_consume_positional_values() {
        for role in [
            ParameterRole::Enter,
            ParameterRole::Context,
            ParameterRole::This(parse_quote!(Object<B>)),
            ParameterRole::ThisRef(parse_quote!(Object<B>)),
            ParameterRole::State(parse_quote!(ModuleState<Settings>)),
        ] {
            assert!(!role.consumes_argument());
        }
    }
}
