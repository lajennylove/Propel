//! Code generation for Propel.
//!
//! Turns a template from `propel_parser` into a Rust function that renders
//! the view against its typed context. The generated code follows the
//! interpreter in `propel_runtime`, and the golden tests in `propel_golden`
//! check that both produce the same bytes.
//!
//! Most apps use [`build::Views`] from a `build.rs` file. See decision 0004.

pub mod build;
mod codegen;
mod references;

pub use references::{references, Reference};

use std::fmt;

use propel_parser::Template;

/// Where the generator reads templates from. `build::Views` reads files and
/// namespaces. Tests can pass an in-memory map.
pub trait Templates {
    /// The parsed template for `name`, or a message that says why it could not be read.
    fn template(&self, name: &str) -> Result<Template, String>;

    /// The file a template is read from, for error messages. A name without a
    /// file of its own gets `<name>.propel`.
    fn path(&self, name: &str) -> String {
        format!("{name}.propel")
    }
}

/// A view that cannot be compiled. It names the template where the problem
/// was found, which may be a layout or a partial that the view uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub template: String,
    pub message: String,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not compile `{}`: {}", self.template, self.message)
    }
}

impl std::error::Error for CompileError {}

/// What the generated code is written against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The Rust type of the view's context, as it is written in the app,
    /// such as `crate::posts::IndexContext`. Its fields must be `pub` if the
    /// type is in another module.
    pub context_type: String,
    /// The path to `propel_runtime` in the generated code. The default works
    /// when the app depends on `propel_runtime` directly.
    pub runtime_path: String,
    /// The asset namespaces the app declares, such as `ignite`. A namespaced
    /// `asset()` or `@vite()` must name one of them, or the build fails. The
    /// app gives each namespace's source at run time, with `Assets::namespace`.
    pub asset_namespaces: Vec<String>,
}

impl Options {
    /// Options for a view with the context type `context_type`.
    pub fn new(context_type: impl Into<String>) -> Self {
        Self {
            context_type: context_type.into(),
            runtime_path: "::propel_runtime".into(),
            asset_namespaces: Vec::new(),
        }
    }
}

/// Generates the `render` function for the view `entry`. The result is Rust
/// source for one function, and it is meant to be placed in a module.
pub fn generate(
    templates: &dyn Templates,
    entry: &str,
    options: &Options,
) -> Result<String, CompileError> {
    codegen::render_fn(templates, entry, options)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Templates held in memory, so the generator can be tested without files.
    struct Map(HashMap<&'static str, &'static str>);

    impl Templates for Map {
        fn template(&self, name: &str) -> Result<Template, String> {
            let source = self
                .0
                .get(name)
                .ok_or_else(|| format!("no template `{name}`"))?;
            propel_parser::parse(source)
                .map_err(|error| format!("could not parse template `{name}`:\n{error}"))
        }
    }

    fn map(templates: &[(&'static str, &'static str)]) -> Map {
        Map(templates.iter().copied().collect())
    }

    fn compile(templates: &Map, entry: &str) -> Result<String, CompileError> {
        generate(templates, entry, &Options::new("crate::Ctx"))
    }

    #[test]
    fn fields_are_read_from_the_context() {
        let code = compile(&map(&[("a", "{{ $title }} {{ $post.title }}")]), "a").unwrap();

        assert!(code.contains("pub fn render(ctx: &crate::Ctx"));
        assert!(code.contains("__site1(&ctx.title)"));
        assert!(code.contains("__site2(&(") && code.contains("Probe(&ctx.post)"));
    }

    #[test]
    fn a_wrong_type_names_the_template_line_and_column() {
        let code = compile(&map(&[("a", "line one\n  {{ $posts }}")]), "a").unwrap();

        assert!(code.contains("message = \"a.propel:2:6: `$posts` cannot be printed in an echo"));
    }

    #[test]
    fn keywords_are_raw_identifiers() {
        let code = compile(&map(&[("a", "{{ $type }}")]), "a").unwrap();

        assert!(code.contains("&ctx.r#type"));
    }

    #[test]
    fn keywords_that_cannot_be_raw_are_errors() {
        let error = compile(&map(&[("a", "{{ $self }}")]), "a").unwrap_err();

        assert_eq!(error.template, "a");
        assert!(error.message.contains("`self` cannot be a field name"));
    }

    #[test]
    fn a_loop_variable_shadows_the_context() {
        let code = compile(
            &map(&[("a", "@foreach (title in $posts) {{ $title }} @endforeach")]),
            "a",
        )
        .unwrap();

        assert!(code.contains("for v_title in __items.iter()"));
        assert!(code.contains("(&v_title)"));
    }

    #[test]
    fn a_missing_include_names_the_template_that_includes_it() {
        let error = compile(&map(&[("a", "@include('gone')")]), "a").unwrap_err();

        assert_eq!(error.template, "a");
        assert!(error.message.contains("no template `gone`"));
    }

    #[test]
    fn a_syntax_error_in_a_layout_names_the_layout() {
        let error = compile(
            &map(&[
                ("a", "@extends('l') @section('x') y @endsection"),
                ("l", "@if (x)"),
            ]),
            "a",
        )
        .unwrap_err();

        assert_eq!(error.template, "l");
        assert!(error.message.contains("could not parse template `l`"));
    }

    #[test]
    fn a_section_inside_an_if_is_an_error() {
        let error = compile(
            &map(&[("a", "@if (x) @section('s') y @endsection @endif")]),
            "a",
        )
        .unwrap_err();

        assert!(error.message.contains("must be at the top level"));
    }

    #[test]
    fn an_extends_cycle_is_an_error() {
        let error =
            compile(&map(&[("a", "@extends('b')"), ("b", "@extends('a')")]), "a").unwrap_err();

        assert!(error.message.contains("@extends chain is too deep"));
    }

    #[test]
    fn an_include_cycle_is_an_error() {
        let error = compile(&map(&[("a", "@include('a')")]), "a").unwrap_err();

        assert!(error.message.contains("@include chain at `a` is too deep"));
    }

    #[test]
    fn a_section_that_yields_itself_is_an_error() {
        let error = compile(
            &map(&[("a", "@section('s') @yield('s') @endsection @yield('s')")]),
            "a",
        )
        .unwrap_err();

        assert!(error.message.contains("refers to itself"));
    }

    #[test]
    fn dump_is_only_emitted_for_debug_builds() {
        let code = compile(&map(&[("a", "@dd($title)")]), "a").unwrap();

        assert!(code.contains("if ::core::cfg!(debug_assertions)"));
        assert!(code.contains("break 'f1;"));
    }

    #[test]
    fn an_undeclared_asset_namespace_fails_the_build() {
        let templates = map(&[("a", "@vite('ghost::resources/css/app.css')")]);

        let error = generate(&templates, "a", &Options::new("crate::Ctx")).unwrap_err();

        assert!(
            error
                .message
                .contains("asset namespace `ghost` is not declared"),
            "{error}"
        );
    }

    #[test]
    fn a_declared_asset_namespace_compiles() {
        let templates = map(&[(
            "a",
            "<img src=\"{{ asset('ui::resources/images/x.svg') }}\">",
        )]);
        let mut options = Options::new("crate::Ctx");
        options.asset_namespaces.push("ui".into());

        assert!(generate(&templates, "a", &options).is_ok());
    }

    #[test]
    fn module_paths_replace_hyphens_and_split_namespaces() {
        assert_eq!(
            build::module_path("docs.vite-tailwind").unwrap(),
            ["docs", "vite_tailwind"]
        );
        assert_eq!(build::module_path("ui::card").unwrap(), ["ui", "card"]);
        assert!(build::module_path("self").is_err());
    }
}
