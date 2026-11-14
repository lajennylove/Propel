//! Limits on template size and loop iterations per render.
//!
//! The limits are process-wide, so every test takes `LOCK` and restores the
//! defaults when it ends.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use propel_runtime::{render, set_limits, FileLoader, Limits, RenderError};
use serde_json::json;

static LOCK: Mutex<()> = Mutex::new(());

struct Guard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Guard {
    fn drop(&mut self) {
        set_limits(Limits::default());
    }
}

fn guard() -> Guard {
    let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_limits(Limits::default());
    Guard(lock)
}

fn views(test: &str, files: &[(&str, String)]) -> FileLoader {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("limits")
        .join(test);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (name, source) in files {
        fs::write(root.join(format!("{name}.propel")), source).unwrap();
    }
    FileLoader::new(root)
}

fn items(n: usize) -> serde_json::Value {
    json!({ "items": (0..n).collect::<Vec<_>>() })
}

#[test]
fn defaults_are_one_mib_and_one_hundred_thousand() {
    let limits = Limits::default();
    assert_eq!(limits.max_template_bytes, 1_048_576);
    assert_eq!(limits.max_loop_iterations, 100_000);
}

#[test]
fn template_over_the_size_limit_is_refused() {
    let _g = guard();
    let loader = views("over", &[("big", "x".repeat(1_048_577))]);
    match render(&loader, "big", &json!({})) {
        Err(RenderError::Load { source, .. }) => {
            assert_eq!(source.kind(), io::ErrorKind::InvalidData);
            let message = source.to_string();
            assert!(message.contains("1048576"), "{message}");
            assert!(message.contains("size limit"), "{message}");
        }
        other => panic!("expected a size error, got {other:?}"),
    }
}

#[test]
fn oversized_template_is_not_read_into_memory() {
    let _g = guard();
    // The message names the file's length, which only a stat gives without
    // reading it.
    let loader = views("unread", &[("big", "x".repeat(2000))]);
    set_limits(Limits {
        max_template_bytes: 10,
        ..Limits::default()
    });
    let error = render(&loader, "big", &json!({})).unwrap_err().to_string();
    assert!(error.contains("2000 bytes"), "{error}");
    assert!(error.contains("limit of 10"), "{error}");
}

#[test]
fn template_exactly_at_the_limit_renders() {
    let _g = guard();
    let loader = views("exact", &[("page", "x".repeat(1_048_576))]);
    let html = render(&loader, "page", &json!({})).unwrap();
    assert_eq!(html.len(), 1_048_576);
}

#[test]
fn loop_past_the_budget_stops() {
    let _g = guard();
    let loader = views(
        "budget",
        &[("page", "@foreach (i in items){{ i }}@endforeach".into())],
    );
    // The page itself costs 2 entries (one to collect stacks, one to render).
    assert!(render(&loader, "page", &items(99_998)).is_ok());
    match render(&loader, "page", &items(99_999)) {
        Err(RenderError::Load { source, .. }) => {
            assert_eq!(source.kind(), io::ErrorKind::TimedOut);
            let message = source.to_string();
            assert!(message.contains("100000"), "{message}");
            assert!(message.contains("render budget"), "{message}");
        }
        other => panic!("expected a budget error, got {other:?}"),
    }
}

#[test]
fn nested_loops_count_every_iteration() {
    let _g = guard();
    set_limits(Limits {
        max_loop_iterations: 50,
        ..Limits::default()
    });
    let loader = views(
        "nested",
        &[(
            "page",
            "@foreach (a in items)@foreach (b in items)x@endforeach@endforeach".into(),
        )],
    );
    // 5 outer + 25 inner = 30, within 50. 7: 7 + 49 = 56, over.
    assert!(render(&loader, "page", &items(5)).is_ok());
    assert!(render(&loader, "page", &items(7)).is_err());
}

#[test]
fn loops_across_an_include_share_one_budget() {
    let _g = guard();
    set_limits(Limits {
        max_loop_iterations: 18,
        ..Limits::default()
    });
    let loader = views(
        "include",
        &[
            ("inner", "@foreach (i in items)x@endforeach".into()),
            (
                "page",
                "@foreach (i in items)@include('inner')@endforeach".into(),
            ),
        ],
    );
    // 3 + 3*3 = 12 loop iterations, 3 include entries, and 3 more entries (the
    // page twice and the include walked once for stacks): exactly 18. 4 + 4*4 = 20 does not, though each loop is under 18.
    assert!(render(&loader, "page", &items(3)).is_ok());
    assert!(render(&loader, "page", &items(4)).is_err());
}

#[test]
fn the_budget_is_per_render() {
    let _g = guard();
    set_limits(Limits {
        max_loop_iterations: 12,
        ..Limits::default()
    });
    let loader = views(
        "per_render",
        &[("page", "@foreach (i in items)x@endforeach".into())],
    );
    for _ in 0..3 {
        assert!(render(&loader, "page", &items(10)).is_ok());
    }
}

#[test]
fn set_limits_changes_both_limits() {
    let _g = guard();
    let loader = views(
        "set",
        &[("page", "@foreach (i in items)x@endforeach".into())],
    );
    assert!(render(&loader, "page", &items(5)).is_ok());
    set_limits(Limits {
        max_template_bytes: 5,
        max_loop_iterations: 2,
    });
    // Size is checked first (the template is longer than 5 bytes).
    let error = render(&loader, "page", &items(1)).unwrap_err().to_string();
    assert!(error.contains("limit of 5"), "{error}");
    set_limits(Limits {
        max_template_bytes: 1000,
        max_loop_iterations: 2,
    });
    let error = render(&loader, "page", &items(3)).unwrap_err().to_string();
    assert!(error.contains("budget of 2"), "{error}");
}

#[test]
fn limits_apply_to_confined_namespaces() {
    let _g = guard();
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("limits/confined");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("page.propel"), "x".repeat(100)).unwrap();
    let loader = FileLoader::new(root.join("none")).confined_namespace("theme", &root, None);
    set_limits(Limits {
        max_template_bytes: 10,
        ..Limits::default()
    });
    assert!(matches!(
        render(&loader, "theme::page", &json!({})),
        Err(RenderError::Load { .. })
    ));
}

#[test]
fn loop_free_include_fan_out_stops_quickly() {
    let _g = guard();
    // Each level includes the next one twice: 2^20 entries without a budget.
    let mut files: Vec<(String, String)> = (0..20)
        .map(|i| {
            (
                format!("p{i}"),
                format!("@include('p{}')@include('p{}')", i + 1, i + 1),
            )
        })
        .collect();
    files.push(("p20".into(), "x".into()));
    files.push(("page".into(), "@include('p0')".into()));
    let files: Vec<(&str, String)> = files.iter().map(|(n, s)| (n.as_str(), s.clone())).collect();
    let loader = views("fan_out", &files);
    let started = std::time::Instant::now();
    match render(&loader, "page", &json!({})) {
        Err(RenderError::Load { source, .. }) => {
            assert_eq!(source.kind(), io::ErrorKind::TimedOut);
            assert!(source.to_string().contains("render budget"), "{source}");
        }
        other => panic!("expected a budget error, got {other:?}"),
    }
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn an_ordinary_page_with_a_few_includes_is_far_under_the_default() {
    let _g = guard();
    let loader = views(
        "ordinary",
        &[
            (
                "layout",
                "@include('nav')@yield('body')@include('foot')".into(),
            ),
            ("nav", "nav".into()),
            ("foot", "foot".into()),
            (
                "page",
                "@extends('layout')@section('body')@include('nav')body@endsection".into(),
            ),
        ],
    );
    set_limits(Limits {
        max_loop_iterations: 20,
        ..Limits::default()
    });
    assert!(render(&loader, "page", &json!({})).is_ok());
}

#[test]
fn loop_free_yield_fan_out_stops_quickly() {
    let _g = guard();
    // Each section yields the next twice: 2^20 renders without a budget.
    let mut page = String::from("@extends('layout')");
    for i in 0..20 {
        page.push_str(&format!(
            "@section('a{i}')@yield('a{}')@yield('a{}')@endsection",
            i + 1,
            i + 1
        ));
    }
    page.push_str("@section('a20')x@endsection");
    let loader = views(
        "yield_fan_out",
        &[("layout", "@yield('a0')".into()), ("page", page)],
    );
    let started = std::time::Instant::now();
    match render(&loader, "page", &json!({})) {
        Err(RenderError::Load { source, .. }) => {
            assert_eq!(source.kind(), io::ErrorKind::TimedOut);
            assert!(source.to_string().contains("render budget"), "{source}");
        }
        other => panic!("expected a budget error, got {other:?}"),
    }
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn a_self_yielding_section_is_an_error_not_a_stack_overflow() {
    let _g = guard();
    let loader = views(
        "self_yield",
        &[
            ("layout", "@yield('a')".into()),
            (
                "page",
                "@extends('layout')@section('a')@yield('a')@endsection".into(),
            ),
        ],
    );
    match render(&loader, "page", &json!({})) {
        Err(_) => {}
        Ok(html) => panic!("expected an error, got {html:?}"),
    }
}
