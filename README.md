# Propel

Propel is a Blade-inspired templating system for [Rocket.rs](https://rocket.rs). You write views in Blade-style syntax, and Propel compiles each view into a typed Rust function when the app builds, so a template that uses a field the handler does not provide fails to build.

Documentation and a working example: [propel.rocketboost.rs](https://propel.rocketboost.rs).

## Install

Add the crates from crates.io. Propel 0.2 uses Rocket 0.5, and `propel_rocket` depends on Rocket 0.5.2 or newer in that series, so Cargo keeps one copy of Rocket:

```toml
[dependencies]
rocket = "0.5.2"
propel_rocket = "0.2.0"
propel_runtime = "0.2.0"
propel_macros = "0.2.0"
serde = { version = "1", features = ["derive"] }

[build-dependencies]
propel_compiler = "0.2.0"
```

`propel_compiler` is a build dependency: `build.rs` uses it to compile the views. `propel_runtime` provides `Assets`, and `propel_macros` provides `#[derive(Truthy)]`. `serde` is needed for the `Serialize` derive on the context structs.

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

`PropelView::render("posts.index", &ctx)` renders the same views from disk, in debug and release builds. It reads `resources/views` and any registered namespace on each request, so an edit or a newly uploaded template shows on the next request. Parsed templates are cached by file modification time and length. Use it for views that exist only at run time, such as CMS themes. A missing view is a `RenderError` (a 500 response), not a panic. Compiled views stay available and are the choice for views known at build time.

### Confining third-party themes

`register_namespace` trusts its templates. For templates you do not trust, such as a CMS theme, use `register_confined_namespace(name, root, parent)` (in `propel_rocket`; `FileLoader::confined_namespace` in `propel_runtime`):

```rust
register_confined_namespace("theme_parent", "themes/parent", None);
register_confined_namespace("theme_child", "themes/child", Some("theme_parent"));
```

Every `@extends`, `@include` and `@push` body is judged by the template that contains it, whatever view the render started from, so an app view that extends a theme layout does not lift the theme's limits. Inside a confined theme:

- A qualified name may name only the template's own namespace or its declared `parent`. Another namespace is refused.
- An unqualified name (`@include('partials.nav')`) is theme-relative, as in WordPress child themes. It never reads the app's `resources/views`. It resolves in the active theme first, then in that theme's parent. The active theme is the most-derived confined namespace in the render chain: the first confined template reached, such as the child theme when `child::page` extends `parent::layout`, or when an app view extends `child::layout`. So a parent layout that includes `partials.nav` shows the child's `partials.nav` when the child has one, and the parent's when it does not. Write `parent::partials.nav` to skip the override.
- `..` segments, absolute paths and symlinks that resolve outside the root are refused (paths are canonicalised before the check).
- `parent` must itself be a confined namespace. If it is missing or not confined, rendering fails with a `RenderError` (checked at render time, not at registration).

Anything refused fails with a `RenderError` and reads nothing outside the roots. `{{ }}` still escapes HTML and `{!! !!}` does not, so only give raw output to templates you accept as authors of that HTML.

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
