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
}

/// Reads `<root>/<name>.propel`, with dots in the name as directory
/// separators. `posts.index` reads `<root>/posts/index.propel`.
#[derive(Debug, Clone)]
pub struct FileLoader {
    root: PathBuf,
}

impl FileLoader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Loader for FileLoader {
    fn source(&self, name: &str) -> io::Result<String> {
        let path = self
            .root
            .join(name.replace('.', "/"))
            .with_extension("propel");
        std::fs::read_to_string(path)
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
    };

    let mut out = String::new();
    renderer.page(name, HashMap::new(), 0, &mut out)?;
    Ok(out)
}

/// `@section` bodies by name.
type Sections = HashMap<String, Vec<Node>>;

struct Renderer<'a, L> {
    loader: &'a L,
    context: Value,
    /// Loop variables, innermost last.
    scopes: Vec<(String, Value)>,
    /// The template being rendered, for error messages.
    template: String,
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

        let mut sections = collect_sections(&template);
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

    fn parse(&self, name: &str) -> Result<Template, RenderError> {
        let source = self
            .loader
            .source(name)
            .map_err(|source| RenderError::Load {
                name: name.to_owned(),
                source,
            })?;

        propel_parser::parse(&source).map_err(|error| RenderError::Parse {
            name: name.to_owned(),
            error: Box::new(error),
        })
    }

    fn nodes(
        &mut self,
        nodes: &[Node],
        sections: &Sections,
        out: &mut String,
    ) -> Result<(), RenderError> {
        for node in nodes {
            match node {
                Node::Text(text) => out.push_str(text),
                Node::Echo { expr, raw } => {
                    let value = self.lookup(expr)?;
                    let text = self.printable(expr, &value)?;
                    if *raw {
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
                    let items = match self.lookup(collection)? {
                        Value::Array(items) => items,
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
                        self.scopes.push((item.clone(), value));
                        let result = self.nodes(body, sections, out);
                        self.scopes.pop();
                        result?;
                    }
                }
                // Layouts are resolved in `page`, so an @extends that is not
                // at the top of a template has nothing to do here.
                Node::Extends(_) => {}
                // A section outside a layout renders in place.
                Node::Section { body, .. } => self.nodes(body, sections, out)?,
                Node::Yield(name) => {
                    if let Some(body) = sections.get(name) {
                        self.nodes(body, sections, out)?;
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

        let root = self
            .scopes
            .iter()
            .rev()
            .find(|(name, _)| name == first)
            .map(|(_, value)| value)
            .or_else(|| self.context.get(first));

        root.and_then(|root| {
            rest.iter()
                .try_fold(root, |value, segment| value.get(segment))
        })
        .cloned()
        .ok_or_else(|| RenderError::Undefined {
            template: self.template.clone(),
            path: expr.path.join("."),
        })
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

/// The top-level `@section`s of a template.
fn collect_sections(template: &Template) -> Sections {
    template
        .nodes
        .iter()
        .filter_map(|node| match node {
            Node::Section { name, body } => Some((name.clone(), body.clone())),
            _ => None,
        })
        .collect()
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
