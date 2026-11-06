//! Rocket integration for Propel.
//!
//! [`PropelView`] renders a view with `propel_runtime` and responds with the
//! HTML. Views are read from `views/`, relative to the working directory, on
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

use propel_runtime::{FileLoader, RenderError};
use rocket::http::{ContentType, Status};
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use serde::Serialize;

/// The directory views are read from, relative to the working directory.
const VIEWS_DIR: &str = "views";

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
    /// Renders the view `name` (such as `posts.index`) with `context`.
    pub fn render<C: Serialize>(name: &str, context: &C) -> Self {
        let loader = FileLoader::new(VIEWS_DIR);
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

                let message = if cfg!(debug_assertions) {
                    error.to_string()
                } else {
                    "Internal Server Error".to_owned()
                };

                Response::build()
                    .status(Status::InternalServerError)
                    .header(ContentType::Plain)
                    .sized_body(message.len(), Cursor::new(message))
                    .ok()
            }
        }
    }
}
