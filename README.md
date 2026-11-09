# Propel

Propel is a Blade-inspired templating system for [Rocket.rs](https://rocket.rs). You write views in Blade-style syntax, and Propel compiles each view into a typed Rust function when the app builds, so a template that uses a field the handler does not provide fails to build.

Documentation and a working example: [propel.rocketboost.rs](https://propel.rocketboost.rs).

## Install

Propel is not on crates.io. Add it from git, with the same Rocket revision Propel is built against:

```toml
[dependencies]
rocket = { git = "https://github.com/rwf2/Rocket", rev = "3a54d079" }
propel_rocket = { git = "https://github.com/lajennylove/Propel", tag = "v0.2.0" }
propel_macros = { git = "https://github.com/lajennylove/Propel", tag = "v0.2.0" }
propel_runtime = { git = "https://github.com/lajennylove/Propel", tag = "v0.2.0" }

[build-dependencies]
propel_compiler = { git = "https://github.com/lajennylove/Propel", tag = "v0.2.0" }
```

## Quick start

Put a view in `resources/views/posts/index.propel`:

```blade
@extends('layouts.app')

@section('content')
  @forelse ($posts as $post)
    <h2>{{ $post.title }}</h2>
  @empty
    <p>No posts yet.</p>
  @endforelse
@endsection
```

Compile it in `build.rs`:

```rust
fn main() {
    propel_compiler::build::Views::new("resources/views")
        .view("posts.index", "crate::views::PostsIndex")
        .build();
}
```

Define the context the view is rendered with, and render it from a route:

```rust
#[derive(serde::Serialize)]
pub struct PostsIndex {
    pub posts: Vec<Post>,
}

mod compiled {
    include!(concat!(env!("OUT_DIR"), "/propel_views.rs"));
}

#[get("/posts")]
fn posts() -> PropelView {
    let ctx = PostsIndex { posts: load_posts() };
    PropelView::from_result(compiled::posts::index::render(&ctx, &Assets::new("public")))
}
```

In a debug build, `PropelView::render` renders the same views from disk on every request, so an edit shows on refresh. A release build renders only the compiled views.

## Features

- **Blade-style syntax:** `{{ }}` (escaped) and `{!! !!}` (raw), `@if`, `@foreach`, `@forelse`, `@extends` and `@section`, `@yield`, `@include`, `@push` and `@stack`, `@vite` and `asset()`.
- **Typed views:** a missing field or a wrong type fails the build, with the template's file, line and column in the message.
- **Assets:** `@vite` and `asset()` read the Vite manifest, with the dev server while it runs. Packages can ship their own assets as a namespace.
- **Namespaced views:** a package registers its views as `name::view`, as Laravel's `loadViewsFrom` does.
- **Escaping on by default:** URLs in `href`, `src` and similar attributes are checked against an allowed list of schemes.

## Crates

| Crate | Purpose |
| --- | --- |
| `propel_rocket` | The `PropelView` responder, for Rocket |
| `propel_compiler` | Compiles views to Rust, through the `build.rs` helper |
| `propel_macros` | `#[derive(Truthy)]` and `#[derive(PropelContext)]` |
| `propel_runtime` | The renderer and the asset resolver |
| `propel_parser` | The template grammar and parser |

## Development

Run the tests and the lints from the repository root:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The benchmark for the renderer is `cargo bench -p propel_runtime`.
