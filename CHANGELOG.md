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

### Changed

- Compiled views (`PropelView::from_result`, `PropelView::compiled`) work as
  before. Only the debug-only restriction on `render` is lifted.
