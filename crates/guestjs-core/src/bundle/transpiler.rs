use std::path::Path;

use oxc::{
    allocator::Allocator,
    codegen::Codegen,
    diagnostics::Diagnostics,
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{
        TransformOptions,
        Transformer,
    },
};

use crate::errors::Error;

pub struct Transpiler;

impl Transpiler {
    pub fn new() -> Self {
        Self
    }

    fn failure(path: &str, diagnostics: &Diagnostics) -> Error {
        Error::bundle(format!(
            "{path}: {}",
            diagnostics
                .errors()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }

    pub fn transpile(
        &self,
        path: &str,
        source: &str,
    ) -> Result<String, Error> {
        let source_type = SourceType::from_path(path).unwrap_or_default();

        if !source_type.is_typescript() {
            return Ok(source.to_owned());
        }

        let allocator = Allocator::default();
        let mut parsed = Parser::new(&allocator, source, source_type).parse();

        if parsed.fatal_error || parsed.diagnostics.has_errors() {
            return Err(Self::failure(path, &parsed.diagnostics));
        }

        let semantic = SemanticBuilder::new()
            .with_enum_eval(true)
            .with_check_syntax_error(true)
            .build(&parsed.program);

        if semantic.diagnostics.has_errors() {
            return Err(Self::failure(path, &semantic.diagnostics));
        }

        let transformed = Transformer::new(
            &allocator,
            Path::new(path),
            &TransformOptions::default(),
        )
        .build_with_scoping(
            semantic.semantic.into_scoping(),
            &mut parsed.program,
        );

        if transformed.diagnostics.has_errors() {
            return Err(Self::failure(path, &transformed.diagnostics));
        }

        Ok(Codegen::new()
            .build(&parsed.program)
            .code)
    }
}

impl Default for Transpiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::bundle::transpiler::Transpiler;

    #[test]
    fn javascript_passes_through_unchanged() {
        assert_eq!(
            Transpiler::new()
                .transpile("main.js", "let x = 1;")
                .unwrap(),
            "let x = 1;",
        );
    }

    #[test]
    fn typescript_types_are_erased() {
        assert!(Transpiler::new()
            .transpile("main.ts", "export const x: number = 1;")
            .unwrap()
            .contains("export const x = 1"));
    }

    #[test]
    fn typescript_enums_are_compiled() {
        assert!(!Transpiler::new()
            .transpile(
                "main.ts",
                "enum Side { Left } export const side = Side.Left;",
            )
            .unwrap()
            .contains("enum Side"));
    }

    #[test]
    fn invalid_typescript_is_a_bundle_error_naming_the_path() {
        assert!(Transpiler::new()
            .transpile("main.ts", "export const = ;")
            .is_err_and(|error| {
                error.is_bundle() && error.to_string().contains("main.ts")
            }));
    }

    #[test]
    fn semantic_syntax_errors_are_bundle_errors() {
        assert!(Transpiler::new()
            .transpile("main.ts", "let value; let value;")
            .is_err_and(|error| {
                error.is_bundle() && error.to_string().contains("main.ts")
            }));
    }
}
