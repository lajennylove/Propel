//! Cost of rendering a page in the dev renderer, and how much of it is parsing.
//!
//! Run with `cargo bench -p propel_runtime`. The dev renderer re-reads and
//! re-parses templates on every request, so `parse` is the part a template
//! cache would remove.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use propel_runtime::{render, FileLoader};
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

fn views() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/views")
}

fn context(count: usize) -> PostsIndex {
    PostsIndex {
        title: "Posts".into(),
        posts: (0..count)
            .map(|i| Post {
                title: format!("Post number {i}"),
                published_at: (i % 3 != 0).then(|| "2026-11-05".into()),
            })
            .collect(),
    }
}

fn bench(c: &mut Criterion) {
    let loader = FileLoader::new(views());
    let page = std::fs::read_to_string(format!("{}/posts/index.propel", views())).unwrap();
    let layout = std::fs::read_to_string(format!("{}/layouts/app.propel", views())).unwrap();

    c.bench_function("parse posts.index and layouts.app", |b| {
        b.iter(|| {
            black_box(propel_parser::parse(black_box(&page)).unwrap());
            black_box(propel_parser::parse(black_box(&layout)).unwrap());
        })
    });

    for count in [10, 100, 1_000] {
        let context = context(count);
        c.bench_function(&format!("render posts.index, {count} posts"), |b| {
            b.iter(|| black_box(render(&loader, "posts.index", black_box(&context)).unwrap()))
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
