//! A path that reads a field of a null value is an error. A null at the end of
//! a path still renders as empty, and a missing field is an error as before.

use std::collections::HashMap;
use std::io;

use propel_runtime::{render, Loader, RenderError};
use serde::Serialize;

/// Templates held in memory.
struct Memory(HashMap<&'static str, &'static str>);

impl Loader for Memory {
    fn source(&self, name: &str) -> io::Result<String> {
        self.0
            .get(name)
            .map(|source| source.to_string())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name.to_owned()))
    }
}

fn loader(source: &'static str) -> Memory {
    Memory(HashMap::from([("page", source)]))
}

#[derive(Serialize)]
struct Author {
    name: String,
}

#[derive(Serialize)]
struct Post {
    title: String,
    author: Option<Author>,
    subtitle: Option<String>,
}

#[derive(Serialize)]
struct Page {
    post: Option<Post>,
}

fn with_author() -> Page {
    Page {
        post: Some(Post {
            title: "Hello".into(),
            author: Some(Author { name: "Ada".into() }),
            subtitle: None,
        }),
    }
}

fn without_author() -> Page {
    Page {
        post: Some(Post {
            title: "Hello".into(),
            author: None,
            subtitle: None,
        }),
    }
}

#[test]
fn a_set_parent_renders_its_field() {
    let html = render(&loader("{{ $post.author.name }}"), "page", &with_author()).unwrap();

    assert_eq!(html, "Ada");
}

#[test]
fn a_null_parent_is_an_error_that_names_the_path() {
    let error = render(
        &loader("{{ $post.author.name }}"),
        "page",
        &without_author(),
    )
    .expect_err("reading a field of null fails");

    assert!(matches!(
        &error,
        RenderError::NullValue { template, path }
            if template == "page" && path == "post.author.name"
    ));
    assert!(error
        .to_string()
        .contains("reads a field of a value that is null"));
}

#[test]
fn a_null_parent_of_a_whole_post_is_an_error() {
    let page = Page { post: None };

    assert!(render(&loader("{{ $post.title }}"), "page", &page).is_err());
}

#[test]
fn a_null_leaf_renders_as_empty() {
    let html = render(&loader("[{{ $post.subtitle }}]"), "page", &with_author()).unwrap();

    assert_eq!(html, "[]");
}

#[test]
fn a_guard_avoids_the_error() {
    let html = render(
        &loader("@if ($post.author)<b>{{ $post.author.name }}</b>@endif"),
        "page",
        &without_author(),
    )
    .unwrap();

    // `@if ($post.author)` reads a field of `post`, which is set. It is false
    // because `author` is null, and the body is skipped.
    assert_eq!(html, "");
}

#[test]
fn a_missing_field_is_still_undefined() {
    let error = render(&loader("{{ $post.nope }}"), "page", &with_author())
        .expect_err("a missing key is an error");

    assert!(matches!(error, RenderError::Undefined { .. }));
}
