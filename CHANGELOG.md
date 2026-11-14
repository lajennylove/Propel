# Changelog

## Unreleased

### Added

- `PropelView::render` now works in release builds. It renders views from disk
  (`resources/views` and registered namespaces) on every request, with parsed
  templates cached by file modification time and length, so templates uploaded
  or edited at run time (such as CMS themes) are seen on the next request. A
  missing view is a `RenderError`, not a panic.

### Changed

- Compiled views (`PropelView::from_result`, `PropelView::compiled`) work as
  before. Only the debug-only restriction on `render` is lifted.
