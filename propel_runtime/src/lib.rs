//! Dev-mode renderer for Propel.
//!
//! Renders a parsed template against a context at request time. The source
//! is read from disk on every render, so template edits show up on the next
//! request without a rebuild. Release builds will render compiled views from
//! `propel_compiler` instead.
//!
//! Context values are dynamic here (`serde_json::Value`), so an undefined
//! variable is a render error. The compiled path reports the same mistake at
//! build time.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use propel_parser::{Branch, Expr, Node, ParseError, Template};
use serde::Serialize;
use serde_json::Value;

/// How deep `@extends` chains may go before rendering gives up. This stops a
/// template that extends itself from recursing forever.
const MAX_LAYOUT_DEPTH: usize = 32;

/// Where template source comes from.
pub trait Loader {
    /// Returns the source for a template name such as `posts.index`.
    fn source(&self, name: &str) -> io::Result<String>;

    /// Returns the parsed template. The default parses the source on every
    /// call. `FileLoader` overrides this with a cache.
    fn template(&self, name: &str) -> Result<Arc<Template>, RenderError> {
        let source = self.source(name).map_err(|source| RenderError::Load {
            name: name.to_owned(),
            source,
        })?;
        parse_named(name, &source)
    }
}

fn parse_named(name: &str, source: &str) -> Result<Arc<Template>, RenderError> {
    propel_parser::parse(source)
        .map(Arc::new)
        .map_err(|error| RenderError::Parse {
            name: name.to_owned(),
            error: Box::new(error),
        })
}

/// A parsed template, and the modification time and size of the file it came
/// from. Both are checked: some file systems store times to the second, and a
/// second save inside that second can still change the size.
#[derive(Clone)]
struct CachedTemplate {
    modified: SystemTime,
    len: u64,
    template: Arc<Template>,
}

/// Parsed templates by file path, shared by every `FileLoader`.
///
/// An entry is reused while the file's modification time is unchanged, so an
/// edit is picked up on the next render. The cost is one `stat` per render,
/// where it used to be a read and a parse.
fn cache() -> &'static Mutex<HashMap<PathBuf, CachedTemplate>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedTemplate>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Reads `<root>/<name>.propel`, with dots in the name as directory
/// separators. `posts.index` reads `<root>/posts/index.propel`.
///
/// Names may only contain ASCII letters, digits, `_` and `-`, separated by dots.
/// A hyphen is part of the file name: `docs.vite-tailwind` reads
/// `docs/vite-tailwind.propel`.
/// Anything else (a leading `/`, `..`, a backslash) is rejected before the
/// file system is touched, so a name can never point outside `root`.
///
/// A namespace is a second root, registered with [`FileLoader::namespace`],
/// as Laravel's `loadViewsFrom` does for packages. `ignite::layouts.app` reads
/// `<namespace root>/layouts/app.propel`. The namespace name follows the same
/// rules as a name segment.
#[derive(Debug, Clone)]
pub struct FileLoader {
    root: PathBuf,
    namespaces: BTreeMap<String, PathBuf>,
}

impl FileLoader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            namespaces: BTreeMap::new(),
        }
    }

    /// Adds a namespace, so `name::view` reads from `root`.
    pub fn namespace(mut self, name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        self.namespaces.insert(name.into(), root.into());
        self
    }

    fn path(&self, name: &str) -> io::Result<PathBuf> {
        let (root, view) = match name.split_once("::") {
            Some((namespace, view)) => {
                if !valid_segments(namespace) {
                    return Err(invalid_name(name));
                }
                let root = self.namespaces.get(namespace).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("unknown view namespace `{namespace}` in `{name}`"),
                    )
                })?;
                (root, view)
            }
            None => (&self.root, name),
        };

        if !valid_segments(view) {
            return Err(invalid_name(name));
        }

        let mut path = root.clone();
        for segment in view.split('.') {
            path.push(segment);
        }
        Ok(path.with_extension("propel"))
    }
}

/// True when every dot-separated part is a non-empty run of letters, digits,
/// `_` or `-`.
fn valid_segments(name: &str) -> bool {
    name.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    })
}

fn invalid_name(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid template name `{name}`"),
    )
}

impl Loader for FileLoader {
    fn source(&self, name: &str) -> io::Result<String> {
        std::fs::read_to_string(self.path(name)?)
    }

    fn template(&self, name: &str) -> Result<Arc<Template>, RenderError> {
        let load = |source| RenderError::Load {
            name: name.to_owned(),
            source,
        };

        let path = self.path(name).map_err(load)?;
        let meta = std::fs::metadata(&path).map_err(load)?;
        let (modified, len) = (meta.modified().map_err(load)?, meta.len());

        // A poisoned lock still holds valid entries, so keep using it.
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = cache.get(&path) {
            if cached.modified == modified && cached.len == len {
                return Ok(Arc::clone(&cached.template));
            }
        }

        let source = std::fs::read_to_string(&path).map_err(load)?;
        let template = parse_named(name, &source)?;
        cache.insert(
            path,
            CachedTemplate {
                modified,
                len,
                template: Arc::clone(&template),
            },
        );
        Ok(template)
    }
}

/// Why a render failed.
#[derive(Debug)]
pub enum RenderError {
    /// The template could not be read.
    Load { name: String, source: io::Error },
    /// The template has a syntax error.
    Parse {
        name: String,
        error: Box<ParseError>,
    },
    /// A variable in the template is not in the context.
    Undefined { template: String, path: String },
    /// A `@foreach` or `@forelse` collection is not a list.
    NotIterable { template: String, path: String },
    /// An echoed value is a list or an object, which has no text form.
    NotPrintable { template: String, path: String },
    /// The `@extends` chain is deeper than `MAX_LAYOUT_DEPTH`.
    LayoutTooDeep { template: String },
    /// `asset()` or `@vite()` names a file the build does not have.
    AssetNotBuilt { template: String, path: String },
    /// `@include` nests deeper than `MAX_LAYOUT_DEPTH`, which means a cycle.
    IncludeTooDeep { template: String },
    /// A `@section` is inside an `@if`, a loop, or another section.
    NestedSection { template: String, section: String },
    /// The context could not be converted to a value.
    Context(serde_json::Error),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Load { name, source } => {
                write!(f, "could not read template `{name}`: {source}")
            }
            RenderError::Parse { name, error } => {
                write!(f, "could not parse template `{name}`:\n{error}")
            }
            RenderError::Undefined { template, path } => {
                write!(f, "undefined variable `{path}` in `{template}`")
            }
            RenderError::NotIterable { template, path } => {
                write!(f, "`{path}` in `{template}` is not a list")
            }
            RenderError::NotPrintable { template, path } => {
                write!(f, "`{path}` in `{template}` is a list or object, not text")
            }
            RenderError::NestedSection { template, section } => {
                write!(
                    f,
                    "@section('{section}') in `{template}` must be at the top level of the template"
                )
            }
            RenderError::AssetNotBuilt { template, path } => {
                write!(
                    f,
                    "asset `{path}` used in `{template}` is not built: run `npm run build`, or `npm run dev` while developing"
                )
            }
            RenderError::IncludeTooDeep { template } => {
                write!(f, "@include chain at `{template}` is too deep (a cycle?)")
            }
            RenderError::LayoutTooDeep { template } => {
                write!(f, "@extends chain at `{template}` is too deep (a cycle?)")
            }
            RenderError::Context(error) => {
                write!(f, "context could not be serialized: {error}")
            }
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders `name` with `context`.
pub fn render<L, C>(loader: &L, name: &str, context: &C) -> Result<String, RenderError>
where
    L: Loader,
    C: Serialize,
{
    render_with_assets(loader, &Assets::new("public"), name, context)
}

/// Renders `name` with `context`, resolving `asset()` and `@vite()` with `assets`.
pub fn render_with_assets<L, C>(
    loader: &L,
    assets: &Assets,
    name: &str,
    context: &C,
) -> Result<String, RenderError>
where
    L: Loader,
    C: Serialize,
{
    let context = serde_json::to_value(context).map_err(RenderError::Context)?;
    let mut renderer = Renderer {
        loader,
        assets,
        context,
        scopes: Vec::new(),
        template: String::new(),
        halted: false,
        section_stack: Vec::new(),
        include_depth: 0,
        stacks: HashMap::new(),
    };

    let mut out = String::new();
    renderer.collect_stacks(name, 0)?;
    renderer.page(name, HashMap::new(), 0, &mut out)?;
    Ok(out)
}

/// A `@section` body. It remembers its template, so an error inside it is
/// reported against the template that wrote it, not against the layout.
#[derive(Clone)]
struct Section {
    template: String,
    nodes: Vec<Node>,
}

/// The definitions of each section name, most derived template first. The
/// first one is what a `@yield` shows. `@parent` shows the next one down.
type Sections = HashMap<String, Vec<Section>>;

struct Renderer<'a, L> {
    loader: &'a L,
    assets: &'a Assets,
    context: Value,
    /// Loop variables, innermost last.
    scopes: Vec<(String, Value)>,
    /// The template being rendered, for error messages.
    template: String,
    /// Set by `@dd`. Rendering stops at the next node, up to the end of the
    /// section or partial that holds it.
    halted: bool,
    /// The section definitions being rendered, innermost last, with their index
    /// in `Sections`. `@parent` uses the top one.
    section_stack: Vec<(String, usize)>,
    /// How deep `@include` is nested right now.
    include_depth: usize,
    /// The `@push` and `@prepend` bodies by stack name, in print order.
    stacks: HashMap<String, Vec<Vec<Node>>>,
}

impl<L: Loader> Renderer<'_, L> {
    /// Renders a page. A child template that `@extends` a layout contributes
    /// its sections to the layout. `overrides` holds sections from a more
    /// derived template, and those win over this template's own sections.
    fn page(
        &mut self,
        name: &str,
        overrides: Sections,
        depth: usize,
        out: &mut String,
    ) -> Result<(), RenderError> {
        if depth > MAX_LAYOUT_DEPTH {
            return Err(RenderError::LayoutTooDeep {
                template: name.to_owned(),
            });
        }

        self.template = name.to_owned();
        let template = self.parse(name)?;

        // The derived templates' definitions come first, so they win.
        let mut sections = overrides;
        for (section_name, section) in collect_sections(name, &template)? {
            sections.entry(section_name).or_default().push(section);
        }

        let parent = template.nodes.iter().find_map(|node| match node {
            Node::Extends(parent) => Some(parent.as_str()),
            _ => None,
        });

        match parent {
            Some(parent) => self.page(parent, sections, depth + 1, out),
            None => self.nodes(&template.nodes, &sections, out),
        }
    }

    fn parse(&self, name: &str) -> Result<Arc<Template>, RenderError> {
        self.loader.template(name)
    }

    fn nodes(
        &mut self,
        nodes: &[Node],
        sections: &Sections,
        out: &mut String,
    ) -> Result<(), RenderError> {
        for node in nodes {
            if self.halted {
                break;
            }
            match node {
                Node::Text(text) => out.push_str(text),
                Node::Asset { path, raw, .. } => {
                    let url = self.assets.url(path).ok_or_else(|| self.not_built(path))?;
                    if *raw {
                        out.push_str(&url);
                    } else {
                        escape_into(&url, out);
                    }
                }
                Node::Vite { entries } => {
                    let html = self
                        .assets
                        .tags(entries)
                        .ok_or_else(|| self.not_built(&entries.join(", ")))?;
                    out.push_str(&html);
                }
                Node::Echo {
                    expr,
                    raw,
                    url_attribute,
                } => {
                    let value = self.lookup(expr)?;
                    let text = self.printable(expr, &value)?;
                    if *url_attribute && !is_safe_url(&text) {
                        // A link to a script or other unsafe scheme is replaced
                        // with a harmless placeholder, even when the echo is raw.
                        out.push('#');
                    } else if *raw {
                        out.push_str(&text);
                    } else {
                        escape_into(&text, out);
                    }
                }
                Node::If {
                    branches,
                    otherwise,
                } => {
                    let mut chosen = None;
                    for Branch { condition, body } in branches {
                        if truthy(&self.lookup(condition)?) {
                            chosen = Some(body.as_slice());
                            break;
                        }
                    }

                    if let Some(body) = chosen.or(otherwise.as_deref()) {
                        self.nodes(body, sections, out)?;
                    }
                }
                Node::Loop {
                    item,
                    collection,
                    body,
                    empty,
                } => {
                    // A null collection (a `None` list) is empty.
                    let items = match self.lookup(collection)? {
                        Value::Array(items) => items,
                        Value::Null => Vec::new(),
                        _ => {
                            return Err(RenderError::NotIterable {
                                template: self.template.clone(),
                                path: collection.path.join("."),
                            })
                        }
                    };

                    if items.is_empty() {
                        if let Some(empty) = empty {
                            self.nodes(empty, sections, out)?;
                        }
                    }

                    for value in items {
                        if self.halted {
                            break;
                        }
                        self.scopes.push((item.clone(), value));
                        let result = self.nodes(body, sections, out);
                        self.scopes.pop();
                        result?;
                    }
                }
                // Layouts are resolved in `page`, so an @extends that is not
                // at the top of a template has nothing to do here.
                Node::Dump { expr, halt } => {
                    // Debug helpers are for development. Release output never
                    // includes them.
                    if cfg!(debug_assertions) {
                        let value = self.lookup(expr)?;
                        out.push_str(&dump_html(expr, &value));
                        self.halted |= *halt;
                    }
                }
                Node::Extends(_) => {}
                // A section only renders where it is shown with `@show`. Otherwise
                // it is a definition for the layout's `@yield`, and it renders nothing.
                Node::Section { name, show, .. } => {
                    if *show {
                        self.render_section(name, 0, sections, out)?;
                    }
                }
                Node::Yield { name, default } => {
                    if is_defined(sections, name) {
                        self.render_section(name, 0, sections, out)?;
                    } else if let Some(default) = default {
                        escape_into(default, out);
                    }
                }
                Node::Include(name) => {
                    if self.include_depth >= MAX_LAYOUT_DEPTH {
                        return Err(RenderError::IncludeTooDeep {
                            template: name.clone(),
                        });
                    }

                    let template = self.parse(name)?;
                    let outer = std::mem::replace(&mut self.template, name.clone());
                    self.include_depth += 1;
                    let result = self.nodes(&template.nodes, sections, out);
                    // `@dd` stops only the partial it is in.
                    self.halted = false;
                    self.include_depth -= 1;
                    self.template = outer;
                    result?;
                }
                // A push renders in its stack, which is printed by `@stack`.
                Node::Push { .. } => {}
                Node::Stack(name) => {
                    let bodies = self.stacks.get(name).cloned().unwrap_or_default();
                    for body in bodies {
                        self.nodes(&body, sections, out)?;
                    }
                }
                Node::Parent => {
                    if let Some((name, index)) = self.section_stack.last().cloned() {
                        self.render_section(&name, index + 1, sections, out)?;
                    }
                }
                Node::HasSection {
                    name,
                    present,
                    body,
                } => {
                    if is_defined(sections, name) == *present {
                        self.nodes(body, sections, out)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Gathers the `@push` and `@prepend` bodies of `name`, its layouts and its
    /// partials, so a `@stack` can print them even when it renders first.
    fn collect_stacks(&mut self, name: &str, depth: usize) -> Result<(), RenderError> {
        if depth > MAX_LAYOUT_DEPTH {
            return Err(RenderError::LayoutTooDeep {
                template: name.to_owned(),
            });
        }

        let template = self.parse(name)?;
        self.collect_pushes(&template.nodes, depth)?;

        let parent = template.nodes.iter().find_map(|node| match node {
            Node::Extends(parent) => Some(parent.clone()),
            _ => None,
        });
        if let Some(parent) = parent {
            self.collect_stacks(&parent, depth + 1)?;
        }

        Ok(())
    }

    fn collect_pushes(&mut self, nodes: &[Node], depth: usize) -> Result<(), RenderError> {
        for node in nodes {
            match node {
                Node::Push {
                    name,
                    body,
                    prepend,
                } => {
                    let stack = self.stacks.entry(name.clone()).or_default();
                    if *prepend {
                        stack.insert(0, body.clone());
                    } else {
                        stack.push(body.clone());
                    }
                    self.collect_pushes(body, depth)?;
                }
                Node::Section { body, .. } | Node::HasSection { body, .. } => {
                    self.collect_pushes(body, depth)?;
                }
                Node::If {
                    branches,
                    otherwise,
                } => {
                    for branch in branches {
                        self.collect_pushes(&branch.body, depth)?;
                    }
                    if let Some(otherwise) = otherwise {
                        self.collect_pushes(otherwise, depth)?;
                    }
                }
                Node::Loop { body, empty, .. } => {
                    self.collect_pushes(body, depth)?;
                    if let Some(empty) = empty {
                        self.collect_pushes(empty, depth)?;
                    }
                }
                Node::Include(name) => {
                    if depth >= MAX_LAYOUT_DEPTH {
                        return Err(RenderError::IncludeTooDeep {
                            template: name.clone(),
                        });
                    }
                    let template = self.parse(name)?;
                    self.collect_pushes(&template.nodes, depth + 1)?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Renders definition `index` of section `name`. Inside it, `@parent`
    /// renders definition `index + 1`. Nothing renders when that does not exist.
    fn render_section(
        &mut self,
        name: &str,
        index: usize,
        sections: &Sections,
        out: &mut String,
    ) -> Result<(), RenderError> {
        let Some(section) = sections.get(name).and_then(|defs| defs.get(index)) else {
            return Ok(());
        };

        let outer = std::mem::replace(&mut self.template, section.template.clone());
        self.section_stack.push((name.to_owned(), index));
        let result = self.nodes(&section.nodes, sections, out);
        // `@dd` stops only the section it is in, so the layout still finishes.
        self.halted = false;
        self.section_stack.pop();
        self.template = outer;
        result
    }

    fn not_built(&self, path: &str) -> RenderError {
        RenderError::AssetNotBuilt {
            template: self.template.clone(),
            path: path.to_owned(),
        }
    }

    /// Finds the value of a path. Loop variables shadow the context.
    fn lookup(&self, expr: &Expr) -> Result<Value, RenderError> {
        let (first, rest) = expr
            .path
            .split_first()
            .expect("the parser never produces an empty path");

        let undefined = || RenderError::Undefined {
            template: self.template.clone(),
            path: expr.path.join("."),
        };

        let mut value = self
            .scopes
            .iter()
            .rev()
            .find(|(name, _)| name == first)
            .map(|(_, value)| value)
            .or_else(|| self.context.get(first))
            .ok_or_else(undefined)?;

        for segment in rest {
            // A null parent makes the whole path null, so an optional
            // relation that is not set renders as empty. A key that is
            // missing from an object is still an error.
            if value.is_null() {
                return Ok(Value::Null);
            }
            value = value.get(segment).ok_or_else(undefined)?;
        }

        Ok(value.clone())
    }

    /// The text for an echoed value. `null` (a `None` in Rust) echoes as
    /// nothing, the same as an empty string.
    fn printable(&self, expr: &Expr, value: &Value) -> Result<String, RenderError> {
        match value {
            Value::Null => Ok(String::new()),
            Value::String(text) => Ok(text.clone()),
            Value::Bool(flag) => Ok(flag.to_string()),
            Value::Number(number) => Ok(number.to_string()),
            Value::Array(_) | Value::Object(_) => Err(RenderError::NotPrintable {
                template: self.template.clone(),
                path: expr.path.join("."),
            }),
        }
    }
}

/// The top-level `@section`s of a template. A section anywhere else (inside
/// an `@if`, a loop, or another section) is an error. It would otherwise be
/// silently dropped, or rendered in an unexpected place.
fn collect_sections(
    name: &str,
    template: &Template,
) -> Result<HashMap<String, Section>, RenderError> {
    let mut sections = HashMap::new();

    for node in &template.nodes {
        match node {
            Node::Section {
                name: section,
                body,
                ..
            } => {
                if let Some(inner) = find_section(body) {
                    return Err(nested_section(name, inner));
                }
                sections.insert(
                    section.clone(),
                    Section {
                        template: name.to_owned(),
                        nodes: body.clone(),
                    },
                );
            }
            other => {
                if let Some(inner) = find_section(std::slice::from_ref(other)) {
                    return Err(nested_section(name, inner));
                }
            }
        }
    }

    Ok(sections)
}

/// The name of the first `@section` found anywhere inside `nodes`.
fn find_section(nodes: &[Node]) -> Option<&str> {
    nodes.iter().find_map(|node| match node {
        Node::Section { name, .. } => Some(name.as_str()),
        Node::HasSection { body, .. } => find_section(body),
        Node::If {
            branches,
            otherwise,
        } => branches
            .iter()
            .find_map(|branch| find_section(&branch.body))
            .or_else(|| otherwise.as_deref().and_then(find_section)),
        Node::Loop { body, empty, .. } => {
            find_section(body).or_else(|| empty.as_deref().and_then(find_section))
        }
        _ => None,
    })
}

/// Whether any template defines `@section` `name`.
fn is_defined(sections: &Sections, name: &str) -> bool {
    sections.get(name).is_some_and(|defs| !defs.is_empty())
}

fn nested_section(template: &str, section: &str) -> RenderError {
    RenderError::NestedSection {
        template: template.to_owned(),
        section: section.to_owned(),
    }
}

/// Truthiness for `@if`. `null`, `false`, zero, and empty strings, lists,
/// and objects are false, the same as a `None` or empty collection in Rust.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn escape_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
}

/// Whether a value is safe to put in a URL attribute such as `href`.
///
/// Relative URLs are safe. An absolute URL is safe only with a scheme from the
/// allow list. Browsers ignore whitespace and control characters inside a
/// scheme (`java&#9;script:`), so those are removed before the scheme is read.
fn is_safe_url(url: &str) -> bool {
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();

    match cleaned.find(':') {
        Some(colon) if !cleaned[..colon].contains(['/', '?', '#']) => {
            matches!(&cleaned[..colon], "http" | "https" | "mailto" | "tel")
        }
        _ => true,
    }
}

/// The HTML for `@dump` and `@dd`, laid out like Symfony's VarDumper: the
/// expression, then the value as a tree. Each array or object is a `<details>`
/// element, so it folds. Every part has a token class, and all text is escaped.
fn dump_html(expr: &Expr, value: &Value) -> String {
    let mut out = String::from(r#"<pre class="propel-dump"><span class="propel-dump-name">"#);
    escape_into(&format!("${}", expr.path.join(".")), &mut out);
    out.push_str("</span>\n");
    dump_node(value, &mut out);
    out.push_str("\n</pre>");
    out
}

/// One value of the tree: a scalar on its own, or a folding container.
fn dump_node(value: &Value, out: &mut String) {
    match value {
        Value::Null => token(out, "propel-dump-literal", "null"),
        Value::Bool(flag) => token(out, "propel-dump-literal", &flag.to_string()),
        Value::Number(number) => token(out, "propel-dump-literal", &number.to_string()),
        Value::String(text) => token(out, "propel-dump-string", &json_string(text)),
        Value::Array(items) => {
            let entries = items
                .iter()
                .enumerate()
                .map(|(index, item)| (index.to_string(), false, item))
                .collect();
            dump_container(&format!("array:{}", items.len()), entries, out);
        }
        Value::Object(map) => {
            let entries = map
                .iter()
                .map(|(key, item)| (json_string(key), true, item))
                .collect();
            dump_container(&format!("array:{}", map.len()), entries, out);
        }
    }
}

/// A folding container. Each entry is `(key, key_is_text, value)`. Arrays use
/// their index as the key, and objects use their field names.
fn dump_container(label: &str, entries: Vec<(String, bool, &Value)>, out: &mut String) {
    token(out, "propel-dump-type", &format!("{label} "));

    if entries.is_empty() {
        token(out, "propel-dump-punct", "[]");
        return;
    }

    out.push_str(r#"<details open class="propel-dump-node"><summary>"#);
    token(out, "propel-dump-punct", "[");
    out.push_str("</summary><div class=\"propel-dump-children\">");
    for (key, is_text, item) in entries {
        out.push_str(r#"<div class="propel-dump-entry">"#);
        token(
            out,
            if is_text {
                "propel-dump-key"
            } else {
                "propel-dump-literal"
            },
            &key,
        );
        token(out, "propel-dump-punct", " => ");
        dump_node(item, out);
        out.push_str("</div>");
    }
    out.push_str("</div>");
    token(out, "propel-dump-punct", "]");
    out.push_str("</details>");
}

/// `<span class="class">text</span>`, with the text escaped.
fn token(out: &mut String, class: &str, text: &str) {
    out.push_str(r#"<span class=""#);
    out.push_str(class);
    out.push_str(r#"">"#);
    escape_into(text, out);
    out.push_str("</span>");
}

/// A string as JSON, with quotes and escapes, before HTML escaping.
fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// Resolves `asset()` and `@vite()` to URLs.
///
/// Two sources, the same convention as Laravel's Vite plugin:
///
/// - **Development.** While `vite` runs, the plugin writes its URL to
///   `public/hot`. Every asset then points at the dev server, and `@vite()`
///   adds the HMR client.
/// - **Production.** After `vite build`, `public/build/manifest.json` maps each
///   entry to its hashed file. Images and fonts are copied to
///   `public/build/resources`, so they resolve by their source path.
///
/// The hot file wins when it exists. Paths are relative to `public`, and are
/// read again on every render, so a rebuild or a new dev server takes effect
/// on the next request.
#[derive(Debug, Clone)]
pub struct Assets {
    public: PathBuf,
}

impl Assets {
    /// `public` is the directory that holds `hot` and `build/`.
    pub fn new(public: impl Into<PathBuf>) -> Self {
        Self {
            public: public.into(),
        }
    }

    /// The dev server URL, when `public/hot` exists.
    fn dev_server(&self) -> Option<String> {
        let url = std::fs::read_to_string(self.public.join("hot")).ok()?;
        Some(url.trim().trim_end_matches('/').to_owned())
    }

    fn manifest(&self) -> Option<Value> {
        let source = std::fs::read_to_string(self.public.join("build/manifest.json")).ok()?;
        serde_json::from_str(&source).ok()
    }

    /// The URL of one file, such as `resources/images/logo.svg`. `None` when
    /// the file is neither in the build nor served by the dev server.
    pub fn url(&self, path: &str) -> Option<String> {
        if !is_relative(path) {
            return None;
        }

        if let Some(server) = self.dev_server() {
            return Some(format!("{server}/{path}"));
        }

        let manifest = self.manifest();
        let file = manifest
            .as_ref()
            .and_then(|manifest| manifest.get(path))
            .and_then(|entry| entry.get("file"))
            .and_then(Value::as_str);
        if let Some(file) = file {
            return Some(format!("/build/{file}"));
        }

        // Images and fonts are copied as they are, so they keep their path.
        self.public
            .join("build")
            .join(path)
            .is_file()
            .then(|| format!("/build/{path}"))
    }

    /// The tags that load `entries`, for `@vite()`. `None` when an entry is
    /// missing from the build.
    pub fn tags(&self, entries: &[String]) -> Option<String> {
        let mut html = String::new();

        if let Some(server) = self.dev_server() {
            html.push_str(&format!(
                "<script type=\"module\" src=\"{server}/@vite/client\"></script>\n"
            ));
            for entry in entries {
                html.push_str(&tag(&format!("{server}/{entry}"), entry));
            }
            return Some(html);
        }

        let manifest = self.manifest()?;
        for entry in entries {
            let info = manifest.get(entry)?;

            // CSS that the entry imports is loaded with it.
            for css in info
                .get("css")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                html.push_str(&tag(&format!("/build/{}", css.as_str()?), "style.css"));
            }

            let file = info.get("file")?.as_str()?;
            html.push_str(&tag(&format!("/build/{file}"), file));
        }

        Some(html)
    }
}

/// Whether `path` stays inside `public`: no leading `/`, and no `..` segment.
fn is_relative(path: &str) -> bool {
    !path.starts_with('/') && !path.split('/').any(|segment| segment == "..")
}

/// A `<link>` for CSS and a module `<script>` for anything else.
fn tag(url: &str, kind: &str) -> String {
    let mut escaped = String::new();
    escape_into(url, &mut escaped);

    if kind.ends_with(".css") {
        format!("<link rel=\"stylesheet\" href=\"{escaped}\">\n")
    } else {
        format!("<script type=\"module\" src=\"{escaped}\"></script>\n")
    }
}
