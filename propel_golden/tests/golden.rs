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
use propel_runtime::{render_with_assets, Assets, FileLoader, RenderError};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden")
}

fn loader() -> FileLoader {
    FileLoader::new(golden_dir().join("views")).namespace("ui", golden_dir().join("packages/ui"))
}

fn assets() -> Assets {
    Assets::new(golden_dir().join("public"))
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

#[cfg(debug_assertions)]
golden!(dump_full, "dump", full, dump);
#[cfg(debug_assertions)]
golden!(halt_full, "halt", full, halt);
