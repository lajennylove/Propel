# Changelog

## Unreleased

### Added

- `PropelView::render` now works in release builds. It renders views from disk
  (`resources/views` and registered namespaces) on every request, with parsed
  templates cached by file modification time and length, so templates uploaded
  or edited at run time (such as CMS themes) are seen on the next request. A
  missing view is a `RenderError`, not a panic.
- `register_confined_namespace(name, root, parent)` (and
  `FileLoader::confined_namespace`) registers a namespace for untrusted
  templates such as CMS themes. Each `@extends`, `@include` and `@push` body is
  judged by the template that contains it. A qualified name may name only the
  template's own namespace or its declared parent (which must be confined).
  An unqualified name is theme-relative: it resolves in the active (most
  derived) theme, then its parent, so a child theme file overrides the
  parent's, and it never reads app views. `..`, absolute paths and symlinks
  that resolve outside the root fail with a `RenderError`.
  `register_namespace` is unchanged.
- `Loader::resolve_reference` and `Loader::enter_theme`, defaulted methods that
  let a loader resolve or refuse an `@extends` or `@include`.
- `Limits` and `set_limits` (also exported from `propel_rocket`) cap what a
  render may use. `max_template_bytes` (default 1 MiB) refuses a larger
  template file from its size, before reading it. `max_loop_iterations`
  (default 100,000) is the render budget: it stops a render that runs more
  `@foreach` or `@forelse` iterations plus template entries (`@include`,
  `@extends` layouts) in total, across nested loops, includes and layouts, so
  a loop-free include fan-out is stopped too. Each rendered section body also
  counts, and a section that yields itself fails at 32 levels. Includes count
  in both the stack walk and the render walk, so the effective include
  allowance is about half the budget. Both are a
  `RenderError::Load` whose message names the limit and its value
  (`io::ErrorKind::InvalidData` for size, `TimedOut` for the budget), because
  `RenderError` is exhaustive. Compiled views are not limited.

### Changed

- Compiled views (`PropelView::from_result`, `PropelView::compiled`) work as
  before. Only the debug-only restriction on `render` is lifted.
