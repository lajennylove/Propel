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

use std::collections::HashMap;
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
/// Names may only contain ASCII letters, digits and `_`, separated by dots.
/// Anything else (a leading `/`, `..`, a backslash) is rejected before the
/// file system is touched, so a name can never point outside `root`.
#[derive(Debug, Clone)]
pub struct FileLoader {
    root: PathBuf,
}

impl FileLoader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, name: &str) -> io::Result<PathBuf> {
        let valid = name.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        });

        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid template name `{name}`"),
            ));
        }

        let mut path = self.root.clone();
        for segment in name.split('.') {
            path.push(segment);
        }
        Ok(path.with_extension("propel"))
    }
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
    let context = serde_json::to_value(context).map_err(RenderError::Context)?;
    let mut renderer = Renderer {
        loader,
        context,
        scopes: Vec::new(),
        template: String::new(),
        halted: false,
    };

    let mut out = String::new();
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

/// `@section` bodies by name.
type Sections = HashMap<String, Section>;

struct Renderer<'a, L> {
    loader: &'a L,
    context: Value,
    /// Loop variables, innermost last.
    scopes: Vec<(String, Value)>,
    /// The template being rendered, for error messages.
    template: String,
    /// Set by `@dd`. Rendering stops at the next node.
    halted: bool,
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

        let mut sections = collect_sections(name, &template)?;
        sections.extend(overrides);

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
                // A section outside a layout renders in place.
                Node::Section { body, .. } => self.nodes(body, sections, out)?,
                Node::Yield(name) => {
                    if let Some(section) = sections.get(name) {
                        let outer = std::mem::replace(&mut self.template, section.template.clone());
                        let result = self.nodes(&section.nodes, sections, out);
                        self.template = outer;
                        result?;
                    }
                }
            }
        }

        Ok(())
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
fn collect_sections(name: &str, template: &Template) -> Result<Sections, RenderError> {
    let mut sections = Sections::new();

    for node in &template.nodes {
        match node {
            Node::Section {
                name: section,
                body,
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

/// The HTML for `@dump` and `@dd`: the expression, its type, and its value as
/// pretty-printed JSON, escaped.
fn dump_html(expr: &Expr, value: &Value) -> String {
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "list",
        Value::Object(_) => "object",
    };
    let json = serde_json::to_string_pretty(value).unwrap_or_default();

    let mut out = String::from(
        r#"<pre style="background:#111;color:#eee;padding:1em;overflow:auto;font-size:13px"><strong>"#,
    );
    escape_into(&format!("${} ({kind})", expr.path.join(".")), &mut out);
    out.push_str("</strong>\n");
    escape_into(&json, &mut out);
    out.push_str("</pre>");
    out
}
