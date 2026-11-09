//! Asset namespaces: a package's built files are a second source, named as
//! views are. A name with `ns::` resolves through that namespace, and the
//! namespace's manifest, hot file and URL prefix apply to it, not the app's.

use std::fs;
use std::path::PathBuf;

use propel_runtime::{render_with_assets, AssetSource, Assets, FileLoader, RenderError};

/// A fresh directory for `test`, so parallel tests do not share files.
fn dir(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("asset_namespaces")
        .join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("build/assets")).unwrap();
    dir
}

const APP_MANIFEST: &str = r#"{
  "resources/css/app.css": { "file": "assets/app-app.css", "isEntry": true }
}"#;

const IGNITE_MANIFEST: &str = r#"{
  "resources/css/app.css": { "file": "assets/ignite-1.css", "isEntry": true },
  "resources/js/app.js": { "file": "assets/ignite-2.js", "css": ["assets/ignite-1.css"], "isEntry": true },
  "resources/images/badge.svg": { "file": "assets/badge-3.svg", "src": "resources/images/badge.svg" }
}"#;

/// The app's own build and the `ignite` package's build, both in directories.
fn two_directories(test: &str) -> (PathBuf, PathBuf, Assets) {
    let app = dir(&format!("{test}-app"));
    fs::write(app.join("build/manifest.json"), APP_MANIFEST).unwrap();
    let ignite = dir(&format!("{test}-ignite"));
    fs::write(ignite.join("build/manifest.json"), IGNITE_MANIFEST).unwrap();
    let assets = Assets::new(&app).namespace("ignite", AssetSource::directory(&ignite));
    (app, ignite, assets)
}

#[test]
fn a_namespaced_name_resolves_in_its_own_build() {
    let (_, _, assets) = two_directories("resolve");

    assert_eq!(
        assets.url("ignite::resources/images/badge.svg").as_deref(),
        Some("/build/assets/badge-3.svg")
    );
    // The app's own name is still its own.
    assert_eq!(
        assets.url("resources/css/app.css").as_deref(),
        Some("/build/assets/app-app.css")
    );
}

#[test]
fn tags_for_a_namespace_come_from_its_manifest() {
    let (_, _, assets) = two_directories("tags");

    let html = assets
        .tags(&["ignite::resources/js/app.js".into()])
        .expect("built");

    assert!(html.contains(r#"<link rel="stylesheet" href="/build/assets/ignite-1.css">"#));
    assert!(html.contains(r#"<script type="module" src="/build/assets/ignite-2.js"></script>"#));
    assert!(
        !html.contains("app-app.css"),
        "the app's manifest is not used"
    );
}

#[test]
fn a_namespace_has_its_own_url_prefix() {
    let app = dir("prefix-app");
    fs::write(app.join("build/manifest.json"), APP_MANIFEST).unwrap();
    let panel = dir("prefix-panel");
    fs::write(panel.join("build/manifest.json"), IGNITE_MANIFEST).unwrap();
    let assets = Assets::new(&app).namespace(
        "ignite",
        AssetSource::directory(&panel).url_prefix("/admin/build/"),
    );

    assert_eq!(
        assets.url("ignite::resources/images/badge.svg").as_deref(),
        Some("/admin/build/assets/badge-3.svg")
    );
    assert_eq!(
        assets.url("resources/css/app.css").as_deref(),
        Some("/build/assets/app-app.css"),
        "the app keeps /build"
    );
}

const MEMORY_MANIFEST: &str = r#"{
  "resources/css/app.css": { "file": "assets/mem-1.css", "isEntry": true }
}"#;

static MEMORY_FILES: &[(&str, &[u8])] = &[
    ("assets/mem-1.css", b"body{}"),
    ("resources/images/logo.svg", b"<svg/>"),
];

#[test]
fn a_memory_source_works_with_no_directory_at_all() {
    // Assets::empty has no default source, so no `public` directory is read.
    let assets = Assets::empty().namespace(
        "ui",
        AssetSource::memory(MEMORY_MANIFEST, MEMORY_FILES).url_prefix("/admin/build"),
    );

    assert_eq!(
        assets.url("ui::resources/css/app.css").as_deref(),
        Some("/admin/build/assets/mem-1.css")
    );
    assert_eq!(
        assets.url("ui::resources/images/logo.svg").as_deref(),
        Some("/admin/build/resources/images/logo.svg")
    );
    assert_eq!(
        assets
            .tags(&["ui::resources/css/app.css".into()])
            .as_deref(),
        Some("<link rel=\"stylesheet\" href=\"/admin/build/assets/mem-1.css\">\n")
    );
    assert_eq!(
        assets.file("ui::resources/images/logo.svg"),
        Some(&b"<svg/>"[..])
    );
    assert_eq!(
        assets.url("resources/css/app.css"),
        None,
        "no default source"
    );
}

#[test]
fn a_memory_source_has_no_dev_server() {
    // A memory source is built, so a `hot` file in the app's directory does not
    // affect it.
    let app = dir("memory-hot");
    fs::write(app.join("hot"), "http://localhost:5173").unwrap();
    let assets =
        Assets::new(&app).namespace("ui", AssetSource::memory(MEMORY_MANIFEST, MEMORY_FILES));

    assert_eq!(
        assets.url("ui::resources/css/app.css").as_deref(),
        Some("/build/assets/mem-1.css")
    );
}

#[test]
fn a_hot_file_in_a_namespace_points_its_assets_at_the_dev_server() {
    let (_, ignite, assets) = two_directories("hot");
    fs::write(ignite.join("hot"), "http://localhost:5174/\n").unwrap();

    assert_eq!(
        assets.url("ignite::resources/images/badge.svg").as_deref(),
        Some("http://localhost:5174/resources/images/badge.svg")
    );
    // The app has no hot file, so its own assets are still built ones.
    assert_eq!(
        assets.url("resources/css/app.css").as_deref(),
        Some("/build/assets/app-app.css")
    );
}

#[test]
fn a_namespaced_vite_renders_in_dev_with_the_namespaces_dev_server() {
    let (_, ignite, assets) = two_directories("vite_dev");
    fs::write(ignite.join("hot"), "http://localhost:5174").unwrap();

    let views = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("asset_namespaces_views")
        .join("vite_dev");
    fs::create_dir_all(&views).unwrap();
    fs::write(
        views.join("page.propel"),
        "@vite('ignite::resources/css/app.css')<img src=\"{{ asset('ignite::resources/images/badge.svg') }}\">",
    )
    .unwrap();

    let html = render_with_assets(
        &FileLoader::new(&views),
        &assets,
        "page",
        &serde_json::json!({}),
    )
    .unwrap();

    assert!(
        html.contains(r#"src="http://localhost:5174/@vite/client""#),
        "{html}"
    );
    assert!(
        html.contains(r#"href="http://localhost:5174/resources/css/app.css""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<img src="http://localhost:5174/resources/images/badge.svg">"#),
        "{html}"
    );
}

#[test]
fn an_undeclared_namespace_at_run_time_is_a_clear_error() {
    let (_, _, assets) = two_directories("undeclared");
    let views = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("asset_namespaces_views")
        .join("undeclared");
    fs::create_dir_all(&views).unwrap();
    fs::write(
        views.join("page.propel"),
        "@vite('ghost::resources/css/app.css')",
    )
    .unwrap();

    let error = render_with_assets(
        &FileLoader::new(&views),
        &assets,
        "page",
        &serde_json::json!({}),
    )
    .expect_err("the namespace was never given");

    assert!(matches!(
        error,
        RenderError::AssetNotBuilt { ref path, .. } if path == "ghost::resources/css/app.css"
    ));
}
