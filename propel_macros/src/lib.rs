//! Proc macros for Propel. See decision 0004.
//!
//! - `#[derive(PropelContext)]` checks a context struct against the views that
//!   use it. A template that reads a field the struct does not have fails the
//!   build, with the template file and line in the message.
//! - `views!` compiles one view from a directory into the current module. It
//!   is for tests, and for an app that does not use a `build.rs`.

use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use propel_compiler::build::Views;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, Attribute, Data, DeriveInput, Fields, Ident, LitStr, Path as SynPath, Token,
};

/// The settings shared by the derive and `views!`.
#[derive(Default)]
struct Settings {
    root: Option<String>,
    namespaces: Vec<(String, String)>,
    views: Vec<String>,
    runtime: Option<String>,
    view: Option<String>,
    context: Option<SynPath>,
}

impl Settings {
    /// The `#[propel(...)]` options of a derive.
    fn from_attributes(attributes: &[Attribute]) -> syn::Result<Self> {
        let mut settings = Settings::default();
        for attribute in attributes.iter().filter(|a| a.path().is_ident("propel")) {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("root") {
                    settings.root = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("runtime") {
                    settings.runtime = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("views") {
                    let names = string_list(meta.input)?;
                    settings.views.extend(names.iter().map(LitStr::value));
                } else if meta.path.is_ident("namespace") {
                    let pair = string_list(meta.input)?;
                    settings.namespaces.push(pair_of(&pair)?);
                } else {
                    return Err(meta.error("unknown propel option"));
                }
                Ok(())
            })?;
        }
        Ok(settings)
    }

    /// The `Views` these settings describe. A relative root is taken from the
    /// crate's manifest directory, as Cargo runs the macro there.
    fn views(&self, root: &str) -> Views {
        let manifest = manifest_dir();
        let resolve = |path: &str| -> PathBuf {
            let path = Path::new(path);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                manifest.join(path)
            }
        };

        let mut views = Views::new(resolve(root)).relative_to(&manifest);
        for (name, path) in &self.namespaces {
            views = views.namespace(name.clone(), resolve(path));
        }
        if let Some(runtime) = &self.runtime {
            views = views.runtime_path(runtime.clone());
        }
        views
    }
}

impl Parse for Settings {
    /// The `views!` arguments: `root = "dir", namespace("ui", "dir"), view = "name", context = Type`.
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut settings = Settings::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "root" => {
                    input.parse::<Token![=]>()?;
                    settings.root = Some(input.parse::<LitStr>()?.value());
                }
                "runtime" => {
                    input.parse::<Token![=]>()?;
                    settings.runtime = Some(input.parse::<LitStr>()?.value());
                }
                "view" => {
                    input.parse::<Token![=]>()?;
                    settings.view = Some(input.parse::<LitStr>()?.value());
                }
                "context" => {
                    input.parse::<Token![=]>()?;
                    settings.context = Some(input.parse()?);
                }
                "namespace" => {
                    let pair = string_list(input)?;
                    settings.namespaces.push(pair_of(&pair)?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown option `{other}`"),
                    ))
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(settings)
    }
}

/// `("a", "b", ...)`, as a list of string literals.
fn string_list(input: ParseStream<'_>) -> syn::Result<Vec<LitStr>> {
    let content;
    syn::parenthesized!(content in input);
    let list = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?;
    Ok(list.into_iter().collect())
}

fn pair_of(list: &[LitStr]) -> syn::Result<(String, String)> {
    match list {
        [name, path] => Ok((name.value(), path.value())),
        _ => Err(syn::Error::new(
            Span::call_site(),
            "a namespace is written namespace(\"name\", \"dir\")",
        )),
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default())
}

/// Checks that every context field a view reads is a field of the struct.
///
/// ```ignore
/// #[derive(Serialize, PropelContext)]
/// #[propel(root = "resources/views", views("posts.index", "posts.show"))]
/// pub struct Basics { pub title: String }
/// ```
///
/// Only the first segment of a path is checked, so `post.title` checks `post`,
/// and a loop variable of the same name is not a field. Nested fields are
/// still checked by rustc when the view is compiled.
#[proc_macro_derive(PropelContext, attributes(propel))]
pub fn derive_propel_context(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_context(&input)
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

fn expand_context(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let settings = Settings::from_attributes(&input.attrs)?;
    let root = settings.root.clone().ok_or_else(|| {
        syn::Error::new_spanned(
            &input.ident,
            "PropelContext needs #[propel(root = \"...\")]",
        )
    })?;
    if settings.views.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "PropelContext needs the views that use this context: #[propel(views(\"name\"))]",
        ));
    }

    let fields = field_names(input)?;
    let views = settings.views(&root);
    let mut errors: Option<syn::Error> = None;

    for view in &settings.views {
        let references = views
            .references(view)
            .map_err(|error| syn::Error::new_spanned(&input.ident, error.to_string()))?;

        for reference in references {
            if fields.iter().any(|field| field == &reference.root) {
                continue;
            }
            let error = syn::Error::new_spanned(
                &input.ident,
                format!(
                    "{}:{}:{}: `${}` is not a field of `{}`, but view `{}` reads it",
                    reference.file,
                    reference.line,
                    reference.column,
                    reference.path,
                    input.ident,
                    view,
                ),
            );
            match errors.as_mut() {
                Some(all) => all.combine(error),
                None => errors = Some(error),
            }
        }
    }

    match errors {
        Some(error) => Err(error),
        None => Ok(TokenStream2::new()),
    }
}

fn field_names(input: &DeriveInput) -> syn::Result<Vec<String>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "PropelContext is for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "PropelContext needs a struct with named fields",
        ));
    };
    Ok(fields
        .named
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| ident.to_string()))
        .collect())
}

/// Compiles one view from a directory into the current module.
///
/// ```ignore
/// views!(root = "tests/views", view = "posts.index", context = Ctx);
/// // posts::index::render(&ctx, &assets) is now available here.
/// ```
#[proc_macro]
pub fn views(input: TokenStream) -> TokenStream {
    let settings = parse_macro_input!(input as Settings);
    expand_views(settings)
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

fn expand_views(settings: Settings) -> syn::Result<TokenStream2> {
    let root = settings
        .root
        .clone()
        .ok_or_else(|| syn::Error::new(Span::call_site(), "views! needs root = \"...\""))?;
    let view = settings
        .view
        .clone()
        .ok_or_else(|| syn::Error::new(Span::call_site(), "views! needs view = \"...\""))?;
    let context = settings
        .context
        .clone()
        .ok_or_else(|| syn::Error::new(Span::call_site(), "views! needs context = Type"))?;

    let text = settings
        .views(&root)
        .view(&view, quote_type(&context))
        .generate()
        .map_err(|error| syn::Error::new(Span::call_site(), error.to_string()))?;
    text.parse::<TokenStream2>()
        .map_err(|error| syn::Error::new(Span::call_site(), error.to_string()))
}

fn quote_type(path: &SynPath) -> String {
    quote::ToTokens::to_token_stream(path).to_string()
}

#[cfg(test)]
mod tests {
    //! The fixtures are the golden crate's `tests/ui/views`, so these tests and
    //! the compile-fail cases read the same templates.

    use super::*;

    const VIEWS: &str = "../propel_golden/tests/ui/views";

    fn context(source: &str) -> DeriveInput {
        syn::parse_str(source).expect("valid struct")
    }

    #[test]
    fn a_missing_field_names_the_template_line_and_column() {
        let input = context(&format!(
            r#"#[propel(root = "{VIEWS}", views("missing_field"))]
            pub struct Ctx {{ pub title: String }}"#
        ));

        let error = expand_context(&input).unwrap_err().to_string();

        assert!(
            error.contains("missing_field.propel:2:7: `$nope` is not a field of `Ctx`"),
            "{error}"
        );
        assert!(error.contains("view `missing_field` reads it"), "{error}");
    }

    #[test]
    fn a_context_with_every_field_passes() {
        let input = context(&format!(
            r#"#[propel(root = "{VIEWS}", views("missing_field"))]
            pub struct Ctx {{ pub title: String, pub nope: u8 }}"#
        ));

        assert!(expand_context(&input).is_ok());
    }

    #[test]
    fn a_field_read_by_a_matching_view_passes() {
        let input = context(&format!(
            r#"#[propel(root = "{VIEWS}", views("not_iterable"))]
            pub struct Ctx {{ pub title: String }}"#
        ));

        assert!(expand_context(&input).is_ok());
    }

    #[test]
    fn a_wrong_type_names_the_template_position_in_the_generated_code() {
        let settings = Settings {
            root: Some(VIEWS.to_owned()),
            view: Some("wrong_type".to_owned()),
            context: Some(syn::parse_str("crate::Ctx").expect("path")),
            ..Settings::default()
        };

        let code = expand_views(settings).expect("generates").to_string();

        assert!(
            code.contains("wrong_type.propel:2:7: `$posts` cannot be printed in an echo"),
            "{code}"
        );
    }

    #[test]
    fn a_loop_over_a_string_names_the_template_position() {
        let settings = Settings {
            root: Some(VIEWS.to_owned()),
            view: Some("not_iterable".to_owned()),
            context: Some(syn::parse_str("crate::Ctx").expect("path")),
            ..Settings::default()
        };

        let code = expand_views(settings).expect("generates").to_string();

        assert!(
            code.contains("not_iterable.propel:2:18: `$title` cannot be looped over"),
            "{code}"
        );
    }

    #[test]
    fn an_unknown_option_is_an_error() {
        let result: syn::Result<Settings> = syn::parse_str("root = \"x\", colour = \"red\"");

        assert!(result.is_err());
    }
}
