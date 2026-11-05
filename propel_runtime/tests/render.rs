use std::fs;

use propel_runtime::{render, FileLoader, RenderError};
use serde::Serialize;

#[derive(Serialize)]
struct Post {
    title: String,
    published_at: Option<String>,
}

#[derive(Serialize)]
struct PostsIndex {
    title: String,
    posts: Vec<Post>,
}

fn loader() -> FileLoader {
    FileLoader::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/views"))
}

fn index(posts: Vec<Post>) -> PostsIndex {
    PostsIndex {
        title: "Posts".into(),
        posts,
    }
}

fn post(title: &str, published_at: Option<&str>) -> Post {
    Post {
        title: title.into(),
        published_at: published_at.map(Into::into),
    }
}

#[test]
fn renders_layout_with_sections() {
    let html = render(
        &loader(),
        "posts.index",
        &index(vec![post("Hello, Propel", Some("2026-11-05"))]),
    )
    .expect("renders");

    // The child's sections fill the layout's yields.
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<title>\n  Posts\n</title>"));
    assert!(html.contains("<h1>Posts</h1>"));
    assert!(html.contains("<h2>Hello, Propel</h2>"));
    assert!(html.contains("<time>2026-11-05</time>"));
    assert!(!html.contains("No posts yet."));
}

#[test]
fn empty_list_renders_the_empty_branch() {
    let html = render(&loader(), "posts.index", &index(vec![])).expect("renders");

    assert!(html.contains("<p>No posts yet.</p>"));
    assert!(!html.contains("<article>"));
}

#[test]
fn null_skips_the_if_and_echoes_nothing() {
    let html =
        render(&loader(), "posts.index", &index(vec![post("Draft", None)])).expect("renders");

    assert!(html.contains("<h2>Draft</h2>"));
    assert!(!html.contains("<time>"));
}

#[test]
fn escaped_echo_escapes_html() {
    let html = render(
        &loader(),
        "posts.index",
        &index(vec![post("<script>alert('x')</script>", None)]),
    )
    .expect("renders");

    assert!(html.contains("<h2>&lt;script&gt;alert(&#x27;x&#x27;)&lt;/script&gt;</h2>"));
    assert!(!html.contains("<script>alert"));
}

#[test]
fn undefined_variable_is_an_error() {
    #[derive(Serialize)]
    struct Wrong {
        heading: String,
    }

    let error = render(
        &loader(),
        "posts.index",
        &Wrong {
            heading: "Posts".into(),
        },
    )
    .expect_err("title is missing");

    assert!(matches!(
        error,
        RenderError::Undefined { ref path, .. } if path == "title"
    ));
}

#[test]
fn missing_template_is_an_error() {
    let error = render(&loader(), "posts.nope", &index(vec![])).expect_err("no such view");
    assert!(matches!(error, RenderError::Load { .. }));
}

#[test]
fn list_is_not_printable() {
    #[derive(Serialize)]
    struct Bad {
        title: Vec<String>,
        posts: Vec<Post>,
    }

    let error = render(
        &loader(),
        "posts.index",
        &Bad {
            title: vec!["a".into()],
            posts: vec![],
        },
    )
    .expect_err("title is a list");

    assert!(matches!(error, RenderError::NotPrintable { .. }));
}

#[test]
fn edits_show_up_without_a_rebuild() {
    // Hot reload: the loader reads from disk on every render.
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("reload");
    fs::create_dir_all(root.join("pages")).unwrap();
    let file = root.join("pages/hello.propel");

    fs::write(&file, "Hello {{ $name }}").unwrap();
    #[derive(Serialize)]
    struct Hello {
        name: &'static str,
    }
    let loader = FileLoader::new(&root);
    assert_eq!(
        render(&loader, "pages.hello", &Hello { name: "Jenny" }).unwrap(),
        "Hello Jenny"
    );

    fs::write(&file, "Hi {{ $name }}!").unwrap();
    assert_eq!(
        render(&loader, "pages.hello", &Hello { name: "Jenny" }).unwrap(),
        "Hi Jenny!"
    );
}

#[test]
fn self_extending_template_is_stopped() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cycle");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("loop.propel"), "@extends('loop')").unwrap();

    let error = render(&FileLoader::new(&root), "loop", &()).expect_err("cycle");
    assert!(matches!(error, RenderError::LayoutTooDeep { .. }));
}
