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

#[test]
fn names_cannot_escape_the_views_directory() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("names");
    fs::create_dir_all(root.join("inside")).unwrap();
    fs::write(root.join("inside/page.propel"), "ok").unwrap();

    // A file outside the views directory, reachable by an absolute name.
    let outside = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("outside-secret");
    fs::write(outside.with_extension("propel"), "secret").unwrap();

    let loader = FileLoader::new(&root);
    assert_eq!(render(&loader, "inside.page", &()).unwrap(), "ok");

    for name in [
        "/abs/path",
        "..",
        "inside..page",
        "inside/page",
        "..\\inside",
        "inside.",
        "",
        outside.to_str().unwrap(),
    ] {
        let error = render(&loader, name, &()).expect_err(name);
        assert!(
            matches!(error, RenderError::Load { ref source, .. } if source.kind() == std::io::ErrorKind::InvalidInput),
            "{name:?} gave {error}"
        );
    }
}

/// Renders `source` as the page `page`, from a directory unique to `test`.
fn render_inline<C: Serialize>(
    test: &str,
    source: &str,
    context: &C,
) -> Result<String, RenderError> {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("inline")
        .join(test);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("page.propel"), source).unwrap();
    render(&FileLoader::new(&root), "page", context)
}

#[test]
fn null_parent_renders_as_empty() {
    let context = serde_json::json!({ "post": { "author": null } });

    let html = render_inline("null_parent", "[{{ $post.author.name }}]", &context).unwrap();
    assert_eq!(html, "[]");
}

#[test]
fn null_list_takes_the_empty_branch() {
    let context = serde_json::json!({ "items": null });

    let html = render_inline(
        "null_list",
        "@forelse (x in items){{ $x }}@empty none@endforelse",
        &context,
    )
    .unwrap();
    assert_eq!(html, " none");
}

#[test]
fn missing_key_under_an_object_is_still_an_error() {
    let context = serde_json::json!({ "post": {} });

    let error =
        render_inline("missing_key", "{{ $post.author }}", &context).expect_err("no author key");
    assert!(matches!(error, RenderError::Undefined { ref path, .. } if path == "post.author"));
}

#[test]
fn errors_in_a_section_name_the_template_that_wrote_it() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("attribution");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("layout.propel"), "<main>@yield('content')</main>").unwrap();
    fs::write(
        root.join("page.propel"),
        "@extends('layout')\n@section('content') {{ $missing }} @endsection",
    )
    .unwrap();

    let error = render(&FileLoader::new(&root), "page", &()).expect_err("missing variable");
    assert!(matches!(
        error,
        RenderError::Undefined { ref template, ref path } if template == "page" && path == "missing"
    ));
}

#[test]
fn section_inside_a_block_is_an_error() {
    let error = render_inline(
        "nested_section",
        "@if (flag) @section('x') y @endsection @endif",
        &serde_json::json!({ "flag": true }),
    )
    .expect_err("section inside @if");

    assert!(matches!(
        error,
        RenderError::NestedSection { ref section, .. } if section == "x"
    ));
}

#[test]
fn unsafe_url_attributes_are_replaced() {
    let context = serde_json::json!({ "u": "javascript:alert(1)" });

    let html = render_inline("unsafe_url", r#"<a href="{{ $u }}">x</a>"#, &context).unwrap();
    assert_eq!(html, r##"<a href="#">x</a>"##);

    // Raw output does not bypass the URL check.
    let html = render_inline("unsafe_url_raw", r#"<a href="{!! $u !!}">x</a>"#, &context).unwrap();
    assert_eq!(html, r##"<a href="#">x</a>"##);
}

#[test]
fn whitespace_inside_a_scheme_does_not_hide_it() {
    let context = serde_json::json!({ "u": "java\tscript:alert(1)" });

    let html = render_inline("tab_scheme", r#"<a href="{{ $u }}">x</a>"#, &context).unwrap();
    assert_eq!(html, r##"<a href="#">x</a>"##);
}

#[test]
fn safe_urls_are_kept() {
    let cases = [
        ("https://example.com/a?b=c", "https://example.com/a?b=c"),
        ("mailto:jenny@example.com", "mailto:jenny@example.com"),
        ("/posts/1", "/posts/1"),
        ("#top", "#top"),
        ("posts/1?x=a:b", "posts/1?x=a:b"),
    ];

    for (input, expected) in cases {
        let context = serde_json::json!({ "u": input });
        let html = render_inline("safe_url", r#"<a href="{{ $u }}">x</a>"#, &context).unwrap();
        assert_eq!(
            html,
            format!(r#"<a href="{}">x</a>"#, expected.replace('&', "&amp;")),
            "{input}"
        );
    }
}

#[test]
fn text_echo_is_not_a_url_check() {
    // Outside an attribute the value is escaped, not replaced.
    let context = serde_json::json!({ "u": "javascript:alert(1)" });
    let html = render_inline("text_echo", "<p>{{ $u }}</p>", &context).unwrap();
    assert_eq!(html, "<p>javascript:alert(1)</p>");
}

#[test]
fn a_second_render_reuses_the_parsed_template() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cache_hit");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("page.propel"), "cached {{ $x }}").unwrap();

    let loader = FileLoader::new(&root);
    let first = propel_runtime::Loader::template(&loader, "page").unwrap();
    let second = propel_runtime::Loader::template(&loader, "page").unwrap();

    assert!(std::sync::Arc::ptr_eq(&first, &second));
}

#[test]
fn an_edit_with_a_new_size_is_seen() {
    // The size changed, so the entry is stale even if the mtime did not move.
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cache_size");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("page.propel");
    fs::write(&file, "ab").unwrap();

    let loader = FileLoader::new(&root);
    assert_eq!(render(&loader, "page", &()).unwrap(), "ab");

    fs::write(&file, "abcd").unwrap();
    assert_eq!(render(&loader, "page", &()).unwrap(), "abcd");
}

#[cfg(debug_assertions)]
#[test]
fn dump_prints_and_rendering_continues() {
    let context = serde_json::json!({ "x": { "a": "<b>" } });
    let html = render_inline("dump", "a @dump($x) b", &context).unwrap();

    assert!(html.starts_with("a <pre"));
    assert!(html.ends_with("</pre> b"));
    assert!(html.contains("$x (object)"));
    assert!(html.contains("&lt;b&gt;"));
}

#[cfg(debug_assertions)]
#[test]
fn dd_prints_and_stops_rendering() {
    let context = serde_json::json!({ "x": 1 });
    let html = render_inline("dd", "a @dd($x) b @dump($x) c", &context).unwrap();

    assert!(html.starts_with("a <pre"));
    assert!(!html.contains(" b "));
    assert_eq!(html.matches("<pre").count(), 1);
}

#[cfg(debug_assertions)]
#[test]
fn dump_of_an_undefined_variable_is_an_error() {
    let error =
        render_inline("dump_missing", "@dump($nope)", &serde_json::json!({})).expect_err("nope");
    assert!(matches!(error, RenderError::Undefined { ref path, .. } if path == "nope"));
}
