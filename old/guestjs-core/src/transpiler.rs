#[cfg(feature = "typescript")]
use std::path::Path;

#[cfg(feature = "typescript")]
use oxc::{
    allocator::Allocator,
    codegen::Codegen,
    diagnostics::Diagnostics,
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{TransformOptions, Transformer},
};

use crate::errors::Error;

/// A guest source transpiler.
pub trait Transpiler {
    fn transpile(&self, name: &str, source: &str) -> Result<String, Error>;
}

/// An oxc-backed guest source transpiler.
#[cfg(feature = "typescript")]
#[derive(Default)]
pub struct OxcTranspiler;

#[cfg(feature = "typescript")]
impl OxcTranspiler {
    fn diagnostics_message(diagnostics: &Diagnostics) -> String {
        diagnostics
            .errors()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(feature = "typescript")]
impl Transpiler for OxcTranspiler {
    fn transpile(&self, name: &str, source: &str) -> Result<String, Error> {
        let source_type = SourceType::from_path(name).unwrap_or_default();

        if !source_type.is_typescript() {
            return Ok(source.to_owned());
        }

        let allocator = Allocator::default();

        let mut parsed = Parser::new(&allocator, source, source_type).parse();

        if parsed.panicked || parsed.diagnostics.has_errors() {
            return Err(Error::transpile(Self::diagnostics_message(&parsed.diagnostics)));
        }

        let scoping = SemanticBuilder::new()
            .with_enum_eval(true)
            .build(&parsed.program)
            .semantic
            .into_scoping();

        let transformed =
            Transformer::new(&allocator, Path::new(name), &TransformOptions::default())
                .build_with_scoping(scoping, &mut parsed.program);

        if transformed.diagnostics.has_errors() {
            return Err(Error::transpile(Self::diagnostics_message(&transformed.diagnostics)));
        }

        Ok(Codegen::new()
            .build(&parsed.program)
            .code)
    }
}
