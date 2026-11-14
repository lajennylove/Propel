//! A confined namespace (a CMS theme) can only reach files inside its own root
//! or its parent's root.

use std::fs;
use std::path::PathBuf;

use propel_runtime::{render, FileLoader, RenderError};

const SECRET: &str = "TOP-SECRET-OUTSIDE-THE-THEME";

/// Writes `files` (name, source) into `<target>/confinement/<test>/<dir>`.
fn write_views(test: &str, dir: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("confinement")
        .join(test)
        .join(dir);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (name, source) in files {
        let path = root.join(format!("{name}.propel"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    root
}

fn is_load_error(result: &Result<String, RenderError>) -> bool {
    matches!(result, Err(RenderError::Load { .. }))
}

fn assert_no_secret(result: &Result<String, RenderError>) {
    if let Ok(html) = result {
        assert!(!html.contains(SECRET), "secret leaked: {html}");
    }
}

#[test]
fn include_with_dot_dot_segments_fails() {
    let app = write_views("dotdot", "app", &[("secrets", SECRET)]);
    let theme = write_views("dotdot", "theme", &[("page", "@include('../../secrets')")]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[test]
fn include_of_an_unqualified_name_does_not_reach_the_app() {
    let app = write_views("unqualified", "app", &[("secrets", SECRET)]);
    let theme = write_views("unqualified", "theme", &[("page", "@include('secrets')")]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[test]
fn extends_of_an_unqualified_name_does_not_reach_the_app() {
    let app = write_views("unq_extends", "app", &[("secrets", SECRET)]);
    let theme = write_views("unq_extends", "theme", &[("page", "@extends('secrets')")]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[test]
fn absolute_paths_fail() {
    let app = write_views("absolute", "app", &[("secrets", SECRET)]);
    let outside = app.join("secrets.propel");
    let source = format!("@include('{}')", outside.display());
    let theme = write_views("absolute", "theme", &[("page", &source)]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[test]
fn another_namespace_that_is_not_the_parent_fails() {
    let other = write_views("other_ns", "other", &[("secrets", SECRET)]);
    let theme = write_views(
        "other_ns",
        "theme",
        &[
            ("page", "@include('other::secrets')"),
            ("layout_user", "@extends('other::secrets')"),
        ],
    );
    let app = write_views("other_ns", "app", &[]);
    let loader = FileLoader::new(app)
        .namespace("other", other)
        .confined_namespace("theme", theme, None);

    for view in ["theme::page", "theme::layout_user"] {
        let result = render(&loader, view, &serde_json::json!({}));
        assert!(is_load_error(&result), "{view}: {result:?}");
        assert_no_secret(&result);
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_that_resolves_outside_the_root_fails() {
    let app = write_views("symlink", "app", &[("secrets", SECRET)]);
    let theme = write_views("symlink", "theme", &[("page", "@include('theme::link')")]);
    std::os::unix::fs::symlink(app.join("secrets.propel"), theme.join("link.propel")).unwrap();
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_that_resolves_outside_the_root_fails() {
    let app = write_views("symdir", "app", &[("secrets", SECRET)]);
    let theme = write_views(
        "symdir",
        "theme",
        &[("page", "@include('theme::dir.secrets')")],
    );
    std::os::unix::fs::symlink(&app, theme.join("dir")).unwrap();
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[cfg(unix)]
#[test]
fn a_symlink_that_stays_inside_the_root_works() {
    let theme = write_views(
        "symlink_ok",
        "theme",
        &[("real", "inside"), ("page", "@include('theme::alias')")],
    );
    std::os::unix::fs::symlink(theme.join("real.propel"), theme.join("alias.propel")).unwrap();
    let app = write_views("symlink_ok", "app", &[]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert_eq!(result.unwrap(), "inside");
}

#[test]
fn a_theme_can_include_and_extend_inside_its_own_root() {
    let theme = write_views(
        "own",
        "theme",
        &[
            ("layout", "<main>@yield('c')</main>"),
            ("part", "part"),
            (
                "page",
                "@extends('theme::layout')@section('c')@include('theme::part')@endsection",
            ),
        ],
    );
    let app = write_views("own", "app", &[]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let result = render(&loader, "theme::page", &serde_json::json!({}));
    assert_eq!(result.unwrap(), "<main>part</main>");
}

#[test]
fn a_child_theme_can_extend_and_include_from_its_parent() {
    let parent = write_views(
        "parent",
        "parent",
        &[
            ("layout", "<main>@yield('c')</main>"),
            ("part", "parent-part"),
        ],
    );
    let child = write_views(
        "parent",
        "child",
        &[(
            "page",
            "@extends('parent::layout')@section('c')@include('parent::part')@endsection",
        )],
    );
    let app = write_views("parent", "app", &[]);
    let loader = FileLoader::new(app)
        .confined_namespace("parent", parent, None)
        .confined_namespace("child", child, Some("parent"));

    let result = render(&loader, "child::page", &serde_json::json!({}));
    assert_eq!(result.unwrap(), "<main>parent-part</main>");
}

#[test]
fn a_parent_theme_cannot_reach_its_child() {
    let parent = write_views("not_down", "parent", &[("page", "@include('child::part')")]);
    let child = write_views("not_down", "child", &[("part", "child-part")]);
    let app = write_views("not_down", "app", &[]);
    let loader = FileLoader::new(app)
        .confined_namespace("parent", parent, None)
        .confined_namespace("child", child, Some("parent"));

    let result = render(&loader, "parent::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
}

#[test]
fn a_child_theme_cannot_reach_its_parents_parent() {
    let grand = write_views("grand", "grand", &[("part", SECRET)]);
    let parent = write_views("grand", "parent", &[]);
    let child = write_views("grand", "child", &[("page", "@include('grand::part')")]);
    let app = write_views("grand", "app", &[]);
    let loader = FileLoader::new(app)
        .confined_namespace("grand", grand, None)
        .confined_namespace("parent", parent, Some("grand"))
        .confined_namespace("child", child, Some("parent"));

    let result = render(&loader, "child::page", &serde_json::json!({}));
    assert!(is_load_error(&result), "{result:?}");
    assert_no_secret(&result);
}

#[test]
fn echo_escapes_html_and_raw_echo_does_not_in_a_confined_namespace() {
    let theme = write_views("escape", "theme", &[("page", "{{ $html }}|{!! $html !!}")]);
    let app = write_views("escape", "app", &[]);
    let loader = FileLoader::new(app).confined_namespace("theme", theme, None);

    let html = render(
        &loader,
        "theme::page",
        &serde_json::json!({"html": "<script>alert(\"x\")</script>"}),
    )
    .unwrap();
    assert_eq!(
        html,
        "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;|<script>alert(\"x\")</script>"
    );
}

#[test]
fn unconfined_namespaces_still_reach_the_app_and_other_namespaces() {
    let app = write_views("legacy", "app", &[("shared", "shared")]);
    let pkg = write_views("legacy", "pkg", &[("page", "@include('shared')")]);
    let loader = FileLoader::new(app).namespace("pkg", pkg);

    let result = render(&loader, "pkg::page", &serde_json::json!({}));
    assert_eq!(result.unwrap(), "shared");
}
