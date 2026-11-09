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

/// A loader that serves views from memory, as a package does when its views
/// are compiled into its binary.
struct Memory(Vec<(&'static str, &'static str)>);

impl propel_runtime::Loader for Memory {
    fn source(&self, name: &str) -> std::io::Result<String> {
        self.0
            .iter()
            .find(|(view, _)| *view == name)
            .map(|(_, source)| source.to_string())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, format!("no view `{name}`"))
            })
    }
}

#[test]
fn a_loader_namespace_renders_views_it_serves() {
    let loader = FileLoader::new(write_views("loader", "app", &[]))
        .namespace_loader("pkg", Memory(vec![("greet", "Hi {{ $name }}")]));

    let html = render(&loader, "pkg::greet", &serde_json::json!({"name": "Ada"}));
    assert_eq!(html.unwrap(), "Hi Ada");
}

#[test]
fn a_loader_namespace_can_be_extended_by_its_own_views() {
    let loader = FileLoader::new(write_views("loader_extends", "app", &[])).namespace_loader(
        "pkg",
        Memory(vec![
            ("layouts.app", "<main>@yield('content')</main>"),
            (
                "page",
                "@extends('pkg::layouts.app')\n@section('content')Body {{ $n }}@endsection",
            ),
        ]),
    );

    let html = render(&loader, "pkg::page", &serde_json::json!({"n": 3}));
    assert_eq!(html.unwrap(), "<main>Body 3</main>");
}

#[test]
fn a_missing_loader_view_is_an_error_that_names_it() {
    let loader = FileLoader::new(write_views("loader_missing", "app", &[]))
        .namespace_loader("pkg", Memory(vec![]));

    let error = render(&loader, "pkg::nothing", &serde_json::json!({})).unwrap_err();
    assert!(matches!(error, RenderError::Load { .. }), "got {error:?}");
}

#[test]
fn path_and_loader_namespaces_coexist() {
    let package = write_views("coexist", "pkg", &[("greet", "File {{ $name }}")]);
    let loader = FileLoader::new(write_views("coexist", "app", &[]))
        .namespace("files", &package)
        .namespace_loader("memory", Memory(vec![("greet", "Memory {{ $name }}")]));

    let file = render(&loader, "files::greet", &serde_json::json!({"name": "A"}));
    let memory = render(&loader, "memory::greet", &serde_json::json!({"name": "B"}));
    assert_eq!(file.unwrap(), "File A");
    assert_eq!(memory.unwrap(), "Memory B");
}
