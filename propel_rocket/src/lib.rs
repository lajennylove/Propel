//! Rocket integration for Propel.
//!
//! [`PropelView`] renders a view with `propel_runtime` and responds with the
//! HTML. Views are read from `resources/views/`, relative to the working directory, on
//! every request, so template edits show up on the next refresh.
//!
//! Release builds are refused on purpose. Production needs the compiled views
//! from `propel_compiler`, which is not built yet. See `docs/production.md`.

#[cfg(not(debug_assertions))]
compile_error!(
    "propel_rocket cannot be built in release mode yet: production needs the compiled \
     views from propel_compiler, which is not built. See propel/docs/production.md."
);

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use propel_runtime::{FileLoader, RenderError};
use rocket::http::{ContentType, Status};
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use serde::Serialize;

/// The directory views are read from, relative to the working directory. As in
/// Laravel, views live in `resources/views`.
const VIEWS_DIR: &str = "resources/views";

/// Namespaces registered with [`register_namespace`], shared by every view.
fn namespaces() -> &'static Mutex<Vec<(String, PathBuf)>> {
    static NAMESPACES: OnceLock<Mutex<Vec<(String, PathBuf)>>> = OnceLock::new();
    NAMESPACES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Makes the views in `root` available as `name::view`, as Laravel's
/// `loadViewsFrom` does for packages. A package's views can then extend its
/// own layout: `@extends('ignite::layouts.app')`.
///
/// Call it once, before the app serves requests, such as from a fairing's
/// `on_ignite`. Registering a name again replaces its root.
pub fn register_namespace(name: impl Into<String>, root: impl Into<PathBuf>) {
    let (name, root) = (name.into(), root.into());
    let mut namespaces = namespaces().lock().unwrap_or_else(|e| e.into_inner());
    namespaces.retain(|(existing, _)| existing != &name);
    namespaces.push((name, root));
}

/// A rendered Propel view. Return it from a route.
///
/// Rendering happens in [`PropelView::render`], so the handler can see the
/// result. A render error becomes a 500 response. The error is logged in every
/// build. The response body shows the message only in debug builds, because
/// it names templates and variables that clients should not see.
pub struct PropelView {
    result: Result<String, RenderError>,
}

impl PropelView {
    /// Renders the view `name` (such as `posts.index`, or `ignite::index` for a
    /// registered namespace) with `context`.
    pub fn render<C: Serialize>(name: &str, context: &C) -> Self {
        let loader = namespaces()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .fold(FileLoader::new(VIEWS_DIR), |loader, (namespace, root)| {
                loader.namespace(namespace.clone(), root.clone())
            });
        PropelView {
            result: propel_runtime::render(&loader, name, context),
        }
    }
}

impl<'r> Responder<'r, 'static> for PropelView {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        match self.result {
            Ok(html) => Response::build()
                .header(ContentType::HTML)
                .sized_body(html.len(), Cursor::new(html))
                .ok(),
            Err(error) => {
                rocket::trace::error!("propel: {error}");

                // Debug builds explain the error in a page. Release builds say
                // nothing about the templates.
                let (content_type, body) = if cfg!(debug_assertions) {
                    (ContentType::HTML, error_page(&error))
                } else {
                    (ContentType::Plain, "Internal Server Error".to_owned())
                };

                Response::build()
                    .status(Status::InternalServerError)
                    .header(content_type)
                    .sized_body(body.len(), Cursor::new(body))
                    .ok()
            }
        }
    }
}

/// The debug page for a render error: the message, a hint about what to do, and
/// the page's style. Everything from the error is escaped.
fn error_page(error: &RenderError) -> String {
    let message = escape(&error.to_string());
    let hint = escape(hint(error));

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Propel: render error</title>
<style>
body {{ font-family: system-ui, sans-serif; max-width: 48rem; margin: 2rem auto; padding: 0 1rem; color: #353841; line-height: 1.7; }}
h1 {{ font-size: 2rem; font-weight: 600; color: #17181c; }}
.hint {{ background: #edeef3; padding: 1rem; }}
pre {{ background: #f6f7f9; padding: 1rem; white-space: pre-wrap; font-size: 14px; }}
</style>
</head>
<body>
<h1>Propel could not render this page</h1>
<p class="hint">{hint}</p>
<pre>{message}</pre>
<p>This page is shown only in debug builds. Release builds show a generic error, and the details go to the log.</p>
</body>
</html>
"#
    )
}

/// What to do about each kind of render error.
fn hint(error: &RenderError) -> &'static str {
    match error {
        RenderError::Undefined { .. } => "The view uses a variable that the handler's context does not have. Add the field to the context struct and set it in the handler, or correct the name in the view.",
        RenderError::NotIterable { .. } => "A loop expects a list. Check that the field is a Vec, or an Option of one, in the context.",
        RenderError::NotPrintable { .. } => "The value is a list or an object, which has no text form. Print one of its fields instead.",
        RenderError::Parse { .. } => "The template has a syntax error. The message points at the line. Check the directive it names, and the @@ escape for a directive word written as text.",
        RenderError::NestedSection { .. } => "A @section must be at the top level of the view. Move it out of the @if or loop, and put the condition inside the section's content.",
        RenderError::AssetNotBuilt { .. } => "The file is not in the build. Run npm run dev while developing, or npm run build before deploying.",
        RenderError::Load { .. } => "The view file could not be read. Check the view name, and that `resources/views` exists where the app runs.",
        RenderError::LayoutTooDeep { .. } => "A chain of @extends refers back to itself. Check the layout names.",
        RenderError::IncludeTooDeep { .. } => "A chain of @include refers back to itself. Check the partial names.",
        RenderError::Context(_) => "The context could not be converted to a value. Check that its struct derives Serialize.",
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_page_escapes_the_message_and_gives_a_hint() {
        let error = RenderError::Undefined {
            template: "<b>page</b>".into(),
            path: "x".into(),
        };
        let page = error_page(&error);

        assert!(page.contains("undefined variable `x` in `&lt;b&gt;page&lt;/b&gt;`"));
        assert!(page.contains("context does not have"));
        assert!(!page.contains("<b>page</b>"));
    }

    #[test]
    fn a_registered_namespace_renders_through_render() {
        #[derive(Serialize)]
        struct Greeting {
            name: &'static str,
        }

        let root = std::env::temp_dir().join(format!("propel-rocket-ns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("greet.propel"), "Hi {{ $name }}").unwrap();

        register_namespace("rocket_test", &root);
        let view = PropelView::render("rocket_test::greet", &Greeting { name: "Ada" });

        assert_eq!(view.result.unwrap(), "Hi Ada");
    }
}
