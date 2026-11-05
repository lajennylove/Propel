//! Rocket integration for Propel.
//!
//! [`PropelView`] renders a view with `propel_runtime` and responds with the
//! HTML. Views are read from `views/`, relative to the working directory, on
//! every request, so template edits show up on the next refresh.

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
/// result. A render error becomes a 500 response that shows the message.
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
                let message = error.to_string();
                Response::build()
                    .status(Status::InternalServerError)
                    .header(ContentType::Plain)
                    .sized_body(message.len(), Cursor::new(message))
                    .ok()
            }
        }
    }
}
