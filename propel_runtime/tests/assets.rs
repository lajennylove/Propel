use std::fs;
use std::path::PathBuf;

use propel_runtime::{render_with_assets, Assets, FileLoader, RenderError};

/// A fresh public directory for `test`, so parallel tests do not share files.
fn public(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("assets")
        .join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("build/assets")).unwrap();
    dir
}

fn manifest() -> &'static str {
    r#"{
      "resources/css/app.css": { "file": "assets/app-1.css", "src": "resources/css/app.css", "isEntry": true },
      "resources/js/app.js": { "file": "assets/app-2.js", "css": ["assets/app-1.css"], "isEntry": true },
      "resources/images/logo.svg": { "file": "assets/logo-3.svg", "src": "resources/images/logo.svg" }
    }"#
}

#[test]
fn production_urls_come_from_the_manifest() {
    let dir = public("prod_urls");
    fs::write(dir.join("build/manifest.json"), manifest()).unwrap();
    let assets = Assets::new(&dir);

    assert_eq!(
        assets.url("resources/images/logo.svg").as_deref(),
        Some("/build/assets/logo-3.svg")
    );
}

#[test]
fn copied_files_resolve_by_their_source_path() {
    let dir = public("copied");
    fs::create_dir_all(dir.join("build/resources/fonts")).unwrap();
    fs::write(dir.join("build/resources/fonts/a.woff2"), "x").unwrap();

    let assets = Assets::new(&dir);
    assert_eq!(
        assets.url("resources/fonts/a.woff2").as_deref(),
        Some("/build/resources/fonts/a.woff2")
    );
}

#[test]
fn dev_server_wins_when_the_hot_file_exists() {
    let dir = public("dev_urls");
    fs::write(dir.join("build/manifest.json"), manifest()).unwrap();
    fs::write(dir.join("hot"), "http://localhost:5173/\n").unwrap();

    let assets = Assets::new(&dir);
    assert_eq!(
        assets.url("resources/images/logo.svg").as_deref(),
        Some("http://localhost:5173/resources/images/logo.svg")
    );
}

#[test]
fn unknown_or_unsafe_paths_have_no_url() {
    let dir = public("unknown");
    fs::write(dir.join("build/manifest.json"), manifest()).unwrap();
    let assets = Assets::new(&dir);

    assert_eq!(assets.url("resources/images/missing.png"), None);
    assert_eq!(assets.url("../../secret.txt"), None);
    assert_eq!(assets.url("/etc/passwd"), None);
}

#[test]
fn production_tags_load_css_and_js_from_the_manifest() {
    let dir = public("prod_tags");
    fs::write(dir.join("build/manifest.json"), manifest()).unwrap();
    let assets = Assets::new(&dir);

    let html = assets
        .tags(&["resources/css/app.css".into(), "resources/js/app.js".into()])
        .unwrap();

    assert!(html.contains(r#"<link rel="stylesheet" href="/build/assets/app-1.css">"#));
    assert!(html.contains(r#"<script type="module" src="/build/assets/app-2.js"></script>"#));
    // The CSS imported by the JS entry is loaded with it.
    assert_eq!(html.matches("app-1.css").count(), 2);
}

#[test]
fn dev_tags_add_the_hmr_client_and_point_at_the_server() {
    let dir = public("dev_tags");
    fs::write(dir.join("hot"), "http://localhost:5173").unwrap();
    let assets = Assets::new(&dir);

    let html = assets.tags(&["resources/css/app.css".into()]).unwrap();

    assert!(html.contains(r#"src="http://localhost:5173/@vite/client""#));
    assert!(html.contains(r#"href="http://localhost:5173/resources/css/app.css""#));
}

#[test]
fn templates_render_asset_and_vite() {
    let dir = public("render");
    fs::write(dir.join("build/manifest.json"), manifest()).unwrap();
    let views = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("assets_views")
        .join("render");
    fs::create_dir_all(&views).unwrap();
    fs::write(
        views.join("page.propel"),
        "@vite(['resources/css/app.css'])<img src=\"{{ asset('resources/images/logo.svg') }}\">",
    )
    .unwrap();

    let html = render_with_assets(
        &FileLoader::new(&views),
        &Assets::new(&dir),
        "page",
        &serde_json::json!({}),
    )
    .unwrap();

    assert!(html.contains(r#"href="/build/assets/app-1.css""#));
    assert!(html.contains(r#"<img src="/build/assets/logo-3.svg">"#));
}

#[test]
fn a_missing_asset_is_a_clear_error() {
    let dir = public("missing_render");
    fs::write(dir.join("build/manifest.json"), manifest()).unwrap();
    let views = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("assets_views")
        .join("missing");
    fs::create_dir_all(&views).unwrap();
    fs::write(
        views.join("page.propel"),
        "{{ asset('resources/images/nope.png') }}",
    )
    .unwrap();

    let error = render_with_assets(
        &FileLoader::new(&views),
        &Assets::new(&dir),
        "page",
        &serde_json::json!({}),
    )
    .expect_err("not built");

    assert!(matches!(
        error,
        RenderError::AssetNotBuilt { ref path, .. } if path == "resources/images/nope.png"
    ));
    assert!(error.to_string().contains("npm run build"));
}
