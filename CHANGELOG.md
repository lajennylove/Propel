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
  templates such as CMS themes. Its `@extends` and `@include` reach only its
  own root and its parent's. `..`, absolute paths, unqualified names, other
  namespaces and symlinks that resolve outside the root fail with a
  `RenderError`. `register_namespace` is unchanged.
- `Loader::check_reference`, a defaulted method that lets a loader refuse an
  `@extends` or `@include`.

### Changed

- Compiled views (`PropelView::from_result`, `PropelView::compiled`) work as
  before. Only the debug-only restriction on `render` is lifted.
