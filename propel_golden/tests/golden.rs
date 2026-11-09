//! Golden tests: each case renders through the dev renderer and through the
//! compiled view, and both must equal the expected file in
//! `golden/expected/<view>.<context>.html`, byte for byte.
//!
//! To write the expected files from the dev renderer, run
//! `PROPEL_UPDATE_GOLDEN=1 cargo test -p propel_golden`, then read every
//! changed file before committing it. The `dump` and `halt` cases depend on
//! debug builds, so they run only there.

use std::fs;
use std::path::PathBuf;

use propel_golden::Basics;
use propel_runtime::{render_with_assets, AssetSource, Assets, FileLoader, RenderError};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden")
}

fn loader() -> FileLoader {
    FileLoader::new(golden_dir().join("views")).namespace("ui", golden_dir().join("packages/ui"))
}

fn assets() -> Assets {
    Assets::new(golden_dir().join("public"))
}

/// The `ui` package's built files, compiled in as a memory source, served
/// under a package-specific prefix. The golden `ns_assets` view uses them.
const UI_MANIFEST: &str = include_str!("../golden/packages/ui/build/manifest.json");
static UI_FILES: &[(&str, &[u8])] = &[
    (
        "assets/ui-1a2b.css",
        include_bytes!("../golden/packages/ui/build/assets/ui-1a2b.css"),
    ),
    (
        "resources/images/badge.svg",
        include_bytes!("../golden/packages/ui/build/resources/images/badge.svg"),
    ),
];

fn assets_with_package() -> Assets {
    assets().namespace(
        "ui",
        AssetSource::memory(UI_MANIFEST, UI_FILES).url_prefix("/admin/build"),
    )
}

/// Renders `view` through the dev renderer and `compiled`, then compares both
/// with the expected file.
fn check(
    view: &str,
    context: &str,
    dev: Result<String, RenderError>,
    compiled: Result<String, RenderError>,
) {
    let dev = dev.unwrap_or_else(|error| panic!("dev renderer failed for {view}: {error}"));
    let compiled =
        compiled.unwrap_or_else(|error| panic!("compiled view failed for {view}: {error}"));
    let path = golden_dir()
        .join("expected")
        .join(format!("{view}.{context}.html"));

    if std::env::var_os("PROPEL_UPDATE_GOLDEN").is_some() {
        fs::write(&path, &dev).expect("writes the expected file");
    }

    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("missing {}: {error}", path.display()));
    assert_eq!(dev, expected, "dev renderer output for {view} ({context})");
    assert_eq!(compiled, expected, "compiled output for {view} ({context})");
}

macro_rules! golden {
    ($test:ident, $view:literal, $context:ident, $module:ident) => {
        #[test]
        fn $test() {
            let context = Basics::$context();
            check(
                $view,
                stringify!($context),
                render_with_assets(&loader(), &assets(), $view, &context),
                propel_golden::$module::render(&context, &assets()),
            );
        }
    };
}

golden!(echo_full, "echo", full, echo);
golden!(echo_empty, "echo", empty, echo);
golden!(conditions_full, "conditions", full, conditions);
golden!(conditions_empty, "conditions", empty, conditions);
golden!(loops_full, "loops", full, loops);
golden!(loops_empty, "loops", empty, loops);
golden!(escaping_full, "escaping", full, escaping);
golden!(layout_full, "layout", full, layout);
golden!(sections_full, "sections", full, sections);
golden!(stacks_full, "stacks", full, stacks);
golden!(includes_full, "includes", full, includes);
golden!(namespaced_full, "namespaced", full, namespaced);
golden!(assets_full, "assets", full, assets);
golden!(nullpath_full, "nullpath", full, nullpath);
golden!(draft_full, "draft", full, draft);

golden!(draft_empty, "draft", empty, draft);
golden!(nullpath_empty, "nullpath", empty, nullpath);

#[cfg(debug_assertions)]
golden!(dump_full, "dump", full, dump);
#[cfg(debug_assertions)]
golden!(halt_full, "halt", full, halt);

/// A path that reads a field of a null value fails in both renderers, with the
/// same message. A `@if` guard avoids it.
#[test]
fn a_null_parent_is_an_error_in_both_renderers() {
    let context = Basics::empty();

    let dev = render_with_assets(&loader(), &assets(), "nullpath_bare", &context)
        .expect_err("the dev renderer reads a field of null");
    let compiled = propel_golden::nullpath_bare::render(&context, &assets())
        .expect_err("the compiled view reads a field of null");

    assert_eq!(dev.to_string(), compiled.to_string());
    assert_eq!(
        dev.to_string(),
        "`latest.title` in `nullpath_bare` reads a field of a value that is null"
    );
}

/// A view that uses a package's assets by namespace renders the same in both
/// renderers. The package's files come from memory, with their own prefix.
#[test]
fn a_namespaced_asset_renders_the_same_in_both_renderers() {
    let context = Basics::full();
    check(
        "ns_assets",
        "full",
        render_with_assets(&loader(), &assets_with_package(), "ns_assets", &context),
        propel_golden::ns_assets::render(&context, &assets_with_package()),
    );
}
