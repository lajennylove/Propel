use std::fs;
use std::path::PathBuf;

use propel_runtime::{render, FileLoader, RenderError};

/// Writes `files` (name, source) into `<target>/namespaces/<test>/<dir>`, and
/// returns that directory.
fn write_views(test: &str, dir: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("namespaces")
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

#[test]
fn a_namespace_view_renders_from_its_own_root() {
    let package = write_views("own_root", "pkg", &[("greet", "Hi {{ $name }}")]);
    let loader = FileLoader::new(write_views("own_root", "app", &[])).namespace("pkg", &package);

    let html = render(&loader, "pkg::greet", &serde_json::json!({"name": "Ada"}));
    assert_eq!(html.unwrap(), "Hi Ada");
}

#[test]
fn a_page_can_extend_its_namespaces_layout() {
    let package = write_views(
        "extends",
        "pkg",
        &[
            ("layouts/app", "<main>@yield('content')</main>"),
            (
                "panel",
                "@extends('pkg::layouts.app')@section('content')Panel@endsection",
            ),
        ],
    );
    let app = write_views("extends", "app", &[("page", "@extends('layouts.app')")]);
    let loader = FileLoader::new(app).namespace("pkg", &package);

    let html = render(&loader, "pkg::panel", &serde_json::json!({}));
    assert_eq!(html.unwrap(), "<main>Panel</main>");
}

#[test]
fn an_app_view_still_reads_the_main_root() {
    let app = write_views("main_root", "app", &[("home", "Home")]);
    let loader = FileLoader::new(app).namespace("pkg", write_views("main_root", "pkg", &[]));

    assert_eq!(
        render(&loader, "home", &serde_json::json!({})).unwrap(),
        "Home"
    );
}

#[test]
fn an_unknown_namespace_is_an_error_that_names_it() {
    let loader = FileLoader::new(write_views("unknown", "app", &[]));

    match render(&loader, "nope::home", &serde_json::json!({})) {
        Err(RenderError::Load { source, .. }) => {
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            assert!(source.to_string().contains("unknown view namespace `nope`"));
        }
        other => panic!("expected a load error, got {other:?}"),
    }
}

#[test]
fn a_namespace_with_a_path_in_it_is_rejected() {
    let package = write_views("traversal", "pkg", &[]);
    let loader = FileLoader::new(write_views("traversal", "app", &[])).namespace("pkg", &package);

    for name in [
        "pkg::../secret",
        "pkg::/etc/passwd",
        "../pkg::home",
        "pkg::",
    ] {
        assert!(
            matches!(
                render(&loader, name, &serde_json::json!({})),
                Err(RenderError::Load { .. })
            ),
            "`{name}` should be rejected"
        );
    }
}
