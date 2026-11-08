use std::fs;
use std::path::PathBuf;

use propel_runtime::{render, FileLoader};

/// Writes `files` (name, source) into a directory unique to `test`, and renders
/// `page` from it with an empty context.
fn render_files(test: &str, files: &[(&str, &str)], page: &str) -> String {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("layouts")
        .join(test);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (name, source) in files {
        fs::write(root.join(format!("{name}.propel")), source).unwrap();
    }
    render(&FileLoader::new(&root), page, &serde_json::json!({})).expect("renders")
}

#[test]
fn yield_prints_its_default_when_no_section_fills_it() {
    let layout = "<title>@yield('title', 'Site')</title>";
    assert_eq!(
        render_files(
            "default_none",
            &[("layout", layout), ("page", "@extends('layout')")],
            "page"
        ),
        "<title>Site</title>"
    );
    assert_eq!(
        render_files(
            "default_some",
            &[
                ("layout", layout),
                (
                    "page",
                    "@extends('layout')\n@section('title') Post @endsection"
                )
            ],
            "page"
        ),
        "<title> Post </title>"
    );
}

#[test]
fn show_renders_the_default_when_a_page_does_not_fill_it() {
    let layout = "<aside>@section('sidebar') Default links @show</aside>";
    assert_eq!(
        render_files(
            "show_default",
            &[("layout", layout), ("page", "@extends('layout')")],
            "page"
        ),
        "<aside> Default links </aside>"
    );
}

#[test]
fn show_renders_the_page_section_when_it_is_filled() {
    let layout = "<aside>@section('sidebar') Default @show</aside>";
    let page = "@extends('layout')\n@section('sidebar') Custom @endsection";
    assert_eq!(
        render_files("show_custom", &[("layout", layout), ("page", page)], "page"),
        "<aside> Custom </aside>"
    );
}

#[test]
fn parent_adds_the_default_content_inside_the_section() {
    let layout = "<aside>@section('sidebar') Default @show</aside>";
    let page = "@extends('layout')\n@section('sidebar') @parent + Extra @endsection";
    assert_eq!(
        render_files("parent", &[("layout", layout), ("page", page)], "page"),
        "<aside>  Default  + Extra </aside>"
    );
}

#[test]
fn parent_works_across_three_levels() {
    let root = "[@section('x') root @show]";
    let middle = "@extends('root')\n@section('x') middle @parent @endsection";
    let child = "@extends('middle')\n@section('x') child @parent @endsection";
    let html = render_files(
        "three_levels",
        &[("root", root), ("middle", middle), ("page", child)],
        "page",
    );
    assert_eq!(
        html.split_whitespace().collect::<Vec<_>>().join(" "),
        "[ child middle root ]"
    );
}

#[test]
fn has_section_renders_only_when_the_page_fills_it() {
    let layout = "@hasSection('sidebar')<aside>@yield('sidebar')</aside>@endif<main>@yield('content')</main>";
    assert_eq!(
        render_files(
            "has_section_yes",
            &[
                ("layout", layout),
                (
                    "page",
                    "@extends('layout')\n@section('sidebar') Links @endsection"
                ),
            ],
            "page"
        ),
        "<aside> Links </aside><main></main>"
    );
    assert_eq!(
        render_files(
            "has_section_no",
            &[("layout", layout), ("page", "@extends('layout')")],
            "page"
        ),
        "<main></main>"
    );
}

#[test]
fn section_missing_renders_only_when_the_page_does_not_fill_it() {
    let layout = "@sectionMissing('sidebar')<p>No sidebar</p>@endif";
    assert_eq!(
        render_files(
            "missing_no",
            &[("layout", layout), ("page", "@extends('layout')")],
            "page"
        ),
        "<p>No sidebar</p>"
    );
    assert_eq!(
        render_files(
            "missing_yes",
            &[
                ("layout", layout),
                (
                    "page",
                    "@extends('layout')\n@section('sidebar') x @endsection"
                ),
            ],
            "page"
        ),
        ""
    );
}

#[test]
fn a_section_without_show_or_yield_renders_nothing() {
    let html = render_files(
        "plain_section",
        &[("page", "before @section('x') hidden @endsection after")],
        "page",
    );
    assert_eq!(html, "before  after");
}

/// Like `render_files`, with a context.
fn render_with(
    test: &str,
    files: &[(&str, &str)],
    page: &str,
    context: &serde_json::Value,
) -> Result<String, propel_runtime::RenderError> {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("layouts")
        .join(test);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (name, source) in files {
        fs::write(root.join(format!("{name}.propel")), source).unwrap();
    }
    render(&FileLoader::new(&root), page, context)
}

#[test]
fn include_renders_a_partial_with_the_same_data() {
    let html = render_with(
        "include_data",
        &[
            ("card", "<h2>{{ $title }}</h2>"),
            ("page", "<main>@include('card')</main>"),
        ],
        "page",
        &serde_json::json!({ "title": "Hello" }),
    )
    .unwrap();
    assert_eq!(html, "<main><h2>Hello</h2></main>");
}

#[test]
fn include_can_read_loop_variables() {
    let html = render_with(
        "include_loop",
        &[
            ("item", "[{{ $tag }}]"),
            ("page", "@foreach (tag in tags)@include('item')@endforeach"),
        ],
        "page",
        &serde_json::json!({ "tags": ["a", "b"] }),
    )
    .unwrap();
    assert_eq!(html, "[a][b]");
}

#[test]
fn include_cycles_are_stopped() {
    let error = render_with(
        "include_cycle",
        &[("loop", "@include('loop')")],
        "loop",
        &serde_json::json!({}),
    )
    .expect_err("cycle");
    assert!(matches!(
        error,
        propel_runtime::RenderError::IncludeTooDeep { .. }
    ));
}

#[test]
fn stack_prints_pushes_from_the_page_and_its_sections() {
    let layout = "<head>@stack('scripts')</head>";
    let page = "@extends('layout')\n@push('scripts') <script>a</script> @endpush\n@section('content') @push('scripts') <script>b</script> @endpush @endsection";
    let html = render_files("stack_basic", &[("layout", layout), ("page", page)], "page");
    assert_eq!(
        html,
        "<head> <script>a</script>  <script>b</script> </head>"
    );
}

#[test]
fn prepend_goes_before_earlier_pushes() {
    let layout = "@stack('scripts')";
    let page = "@extends('layout')\n@push('scripts')A@endpush\n@push('scripts')B@endpush\n@prepend('scripts')C@endprepend";
    let html = render_files(
        "stack_prepend",
        &[("layout", layout), ("page", page)],
        "page",
    );
    assert_eq!(html, "CAB");
}

#[test]
fn empty_stack_prints_nothing() {
    let html = render_files(
        "stack_empty",
        &[
            ("layout", "[@stack('none')]"),
            ("page", "@extends('layout')"),
        ],
        "page",
    );
    assert_eq!(html, "[]");
}

#[cfg(debug_assertions)]
#[test]
fn dd_stops_its_section_and_the_layout_still_finishes() {
    let layout = "<main>@yield('content')</main><footer>end</footer>";
    let page = "@extends('layout')\n@section('content') before @dd($x) after @endsection";
    let html = render_with(
        "dd_scope",
        &[("layout", layout), ("page", page)],
        "page",
        &serde_json::json!({ "x": 1 }),
    )
    .unwrap();

    assert!(html.starts_with("<main> before <pre"));
    assert!(!html.contains(" after "));
    assert!(html.ends_with("</pre></main><footer>end</footer>"));
}
