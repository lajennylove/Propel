//! Turns one entry view and everything it uses into a Rust function.
//!
//! The rules follow `propel_runtime`'s interpreter: `page`, `render_section`,
//! `collect_stacks`, `collect_pushes`, and the `Include` arm of `nodes`. Each
//! use site is inlined, so the function has no run-time lookup. A halt
//! (`@dd`) breaks out of the innermost fragment, which is the page, a
//! section, or an include, the same way the interpreter's `halted` flag stops
//! output until the fragment ends.

use std::collections::HashMap;

use propel_parser::{Branch, Expr, Node, Template};

use crate::{CompileError, Options, Templates};

/// How deep layouts, includes, sections and stacks may nest before the
/// generator gives up. This matches the interpreter's limit.
const MAX_DEPTH: usize = 32;

/// Words that cannot be a Rust field name without `r#`.
pub(crate) const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "unsized", "use",
    "virtual", "where", "while", "yield",
];

/// A `@section` definition, and the template it was written in. Errors inside
/// it name that template.
#[derive(Clone)]
struct Definition {
    template: String,
    nodes: Vec<Node>,
}

/// Definitions by section name, most derived first.
type Sections = HashMap<String, Vec<Definition>>;

/// Generates the `render` function for the view `entry`.
pub(crate) fn render_fn(
    templates: &dyn Templates,
    entry: &str,
    options: &Options,
) -> Result<String, CompileError> {
    let mut gen = Gen {
        templates,
        options,
        code: String::new(),
        labels: 0,
        template: entry.to_owned(),
        fragment: String::new(),
        loops: Vec::new(),
        sections: HashMap::new(),
        section_stack: Vec::new(),
        stacks: HashMap::new(),
        include_depth: 0,
        stack_depth: 0,
        sites: 0,
    };
    gen.page(entry)?;

    let rt = &options.runtime_path;
    let ctx = &options.context_type;
    Ok(format!(
        "#[allow(unused_mut, unused_variables, unused_labels, unreachable_code, non_camel_case_types, non_snake_case, clippy::all)]\n\
         pub fn render(ctx: &{ctx}, assets: &{rt}::Assets) -> Result<::std::string::String, {rt}::RenderError> {{\n\
         #[allow(unused_imports)]\n\
         use {rt}::support::{{NullableProbe as _, PlainProbe as _}};\n\
         let _ = (ctx, assets);\n\
         let mut out = ::std::string::String::new();\n\
         {body}\
         Ok(out)\n\
         }}\n",
        body = gen.code,
    ))
}

struct Gen<'a> {
    templates: &'a dyn Templates,
    options: &'a Options,
    /// The Rust statements generated so far.
    code: String,
    /// The number of labelled blocks, so each one has a unique name.
    labels: usize,
    /// The template being generated, for error messages.
    template: String,
    /// The label of the innermost fragment. `@dd` breaks out of it.
    fragment: String,
    /// Loop variables in scope, innermost last.
    loops: Vec<String>,
    /// The section definitions for the page being generated.
    sections: Sections,
    /// The definitions being generated, innermost last, with their index.
    section_stack: Vec<(String, usize)>,
    /// The `@push` and `@prepend` bodies by stack name, in print order.
    stacks: HashMap<String, Vec<Vec<Node>>>,
    include_depth: usize,
    stack_depth: usize,
    /// The number of type-check sites so far, so each has a unique name.
    sites: usize,
}

/// What a use site needs from its value. Each kind has its own trait, so a
/// wrong type fails with a message that names the template position.
#[derive(Clone, Copy)]
enum Site {
    /// `{{ }}` and `{!! !!}`: the value must have text.
    Echo,
    /// `@if` and `@elseif`: the value must have truthiness.
    Condition,
    /// `@foreach` and `@forelse`: the value must be a list.
    Items,
}

impl Gen<'_> {
    fn error(&self, message: impl Into<String>) -> CompileError {
        CompileError {
            template: self.template.clone(),
            message: message.into(),
        }
    }

    fn load(&self, name: &str) -> Result<Template, CompileError> {
        self.templates
            .template(name)
            .map_err(|message| self.error(message))
    }

    fn emit(&mut self, line: &str) {
        self.code.push_str(line);
        self.code.push('\n');
    }

    /// Emits a trait that the value's type must implement for `kind`, with a
    /// message that names the template, line and column of `expr`. Returns the
    /// name of the checking function and the Rust place of the value.
    fn site(&mut self, kind: Site, expr: &Expr) -> Result<(String, String), CompileError> {
        let place = self.place(expr)?;
        self.sites += 1;
        let number = self.sites;
        let rt = self.options.runtime_path.clone();
        let location = format!(
            "{}:{}:{}",
            self.templates.path(&self.template),
            expr.line,
            expr.column
        );
        let dotted = expr.path.join(".");

        let (message, note, supertrait, returns, body) = match kind {
            Site::Echo => (
                format!("{location}: `${dotted}` cannot be printed in an echo, because it has no text form"),
                "print a text, number or boolean field instead, or implement Printable for the type",
                format!("{rt}::support::Printable"),
                "::std::string::String".to_owned(),
                format!("{rt}::support::Printable::text(value)"),
            ),
            Site::Condition => (
                format!("{location}: `${dotted}` cannot be the condition of @if, because its type has no truthiness"),
                "use a bool, or implement Truthy for the type (true for a struct)",
                format!("{rt}::support::Truthy"),
                "bool".to_owned(),
                format!("{rt}::support::Truthy::truthy(value)"),
            ),
            Site::Items => (
                format!("{location}: `${dotted}` cannot be looped over, because it is not a list"),
                "use a Vec, or an Option of a Vec",
                format!("{rt}::support::Items"),
                format!("&'a [<T as {rt}::support::Items>::Item]"),
                format!("{rt}::support::Items::items(value)"),
            ),
        };

        let name = format!("__Site{number}");
        let func = format!("__site{number}");
        self.emit(&format!(
            "#[diagnostic::on_unimplemented(message = {}, note = {})]",
            literal(&message),
            literal(note)
        ));
        self.emit(&format!("trait {name}: {supertrait} {{}}"));
        self.emit(&format!("impl<T: {supertrait} + ?Sized> {name} for T {{}}"));
        self.emit(&format!(
            "fn {func}<'a, T: {name} + ?Sized>(value: &'a T) -> {returns} {{ {body} }}"
        ));
        Ok((func, place))
    }

    fn new_label(&mut self) -> String {
        self.labels += 1;
        format!("'f{}", self.labels)
    }

    /// Opens a labelled fragment, runs `body` inside it, and closes it.
    fn fragment(
        &mut self,
        template: &str,
        body: impl FnOnce(&mut Self) -> Result<(), CompileError>,
    ) -> Result<(), CompileError> {
        let outer_template = std::mem::replace(&mut self.template, template.to_owned());
        let label = self.new_label();
        let outer_fragment = std::mem::replace(&mut self.fragment, label.clone());
        self.emit(&format!("{label}: {{"));
        let result = body(self);
        self.emit("}");
        self.fragment = outer_fragment;
        self.template = outer_template;
        result
    }

    fn page(&mut self, entry: &str) -> Result<(), CompileError> {
        // Walk the `@extends` chain from the entry view to the root layout.
        let mut chain: Vec<(String, Template)> = Vec::new();
        let mut sections: Sections = HashMap::new();
        let mut name = entry.to_owned();
        loop {
            if chain.len() > MAX_DEPTH {
                self.template = name.clone();
                return Err(self.error("@extends chain is too deep (a cycle?)"));
            }
            self.template = name.clone();
            let template = self.load(&name)?;
            for (section, definition) in collect_sections(&name, &template.nodes)? {
                sections.entry(section).or_default().push(definition);
            }
            let parent = template.nodes.iter().find_map(|node| match node {
                Node::Extends(parent) => Some(parent.clone()),
                _ => None,
            });
            chain.push((name.clone(), template));
            match parent {
                Some(parent) => name = parent,
                None => break,
            }
        }

        // Stacks are collected from the whole chain before anything renders,
        // as `collect_stacks` does, so `@stack` can print pushes that come later.
        self.stacks = HashMap::new();
        for (name, template) in &chain {
            self.template = name.clone();
            self.collect_pushes(&template.nodes, 0)?;
        }

        let (root_name, root) = chain.pop().expect("the chain has the entry view");
        self.sections = sections;
        self.fragment = String::new();
        self.fragment(&root_name, |gen| gen.nodes(&root.nodes))
    }

    /// Mirrors `Renderer::collect_pushes`: every push in the template, its
    /// sections, its conditions and loops, and the partials it includes.
    fn collect_pushes(&mut self, nodes: &[Node], depth: usize) -> Result<(), CompileError> {
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
                    if depth >= MAX_DEPTH {
                        return Err(self
                            .error(format!("@include chain at `{name}` is too deep (a cycle?)")));
                    }
                    let template = self.load(name)?;
                    self.collect_pushes(&template.nodes, depth + 1)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn nodes(&mut self, nodes: &[Node]) -> Result<(), CompileError> {
        for node in nodes {
            self.node(node)?;
        }
        Ok(())
    }

    fn node(&mut self, node: &Node) -> Result<(), CompileError> {
        let rt = self.options.runtime_path.clone();
        match node {
            Node::Text(text) => {
                let line = format!("out.push_str({});", literal(text));
                self.emit(&line);
            }
            Node::Echo {
                expr,
                raw,
                url_attribute,
            } => {
                let (func, place) = self.site(Site::Echo, expr)?;
                self.emit(&format!(
                    "{{ let __t = {func}(&{place}); \
                     {rt}::support::echo(&mut out, &__t, {raw}, {url_attribute}); }}"
                ));
            }
            Node::Asset { path, raw, .. } => {
                self.check_asset_namespace(path)?;
                let error = self.not_built(path);
                self.emit(&format!(
                    "{{ let __u = assets.url({}).ok_or_else(|| {error})?; \
                     {rt}::support::url(&mut out, &__u, {raw}); }}",
                    literal(path)
                ));
            }
            Node::Vite { entries } => {
                for entry in entries {
                    self.check_asset_namespace(entry)?;
                }
                let list: Vec<String> = entries
                    .iter()
                    .map(|entry| format!("{}.to_owned()", literal(entry)))
                    .collect();
                let error = self.not_built(&entries.join(", "));
                self.emit(&format!(
                    "{{ let __h = assets.tags(&[{}]).ok_or_else(|| {error})?; \
                     out.push_str(&__h); }}",
                    list.join(", ")
                ));
            }
            Node::If {
                branches,
                otherwise,
            } => self.if_block(branches, otherwise.as_deref())?,
            Node::Loop {
                item,
                collection,
                body,
                empty,
            } => {
                let (func, collection) = self.site(Site::Items, collection)?;
                self.emit("{");
                self.emit(&format!("let __items = {func}(&{collection});"));
                if let Some(empty) = empty.as_ref().filter(|nodes| !nodes.is_empty()) {
                    self.emit("if __items.is_empty() {");
                    self.nodes(empty)?;
                    self.emit("}");
                }
                self.emit(&format!("for v_{item} in __items.iter() {{"));
                self.loops.push(item.clone());
                let result = self.nodes(body);
                self.loops.pop();
                result?;
                self.emit("}");
                self.emit("}");
            }
            // Layouts are resolved in `page`. An `@extends` anywhere else is ignored.
            Node::Extends(_) => {}
            Node::Section { name, show, .. } => {
                if *show {
                    self.render_section(name, 0)?;
                }
            }
            Node::Yield { name, default } => {
                if self.is_defined(name) {
                    self.render_section(name, 0)?;
                } else if let Some(default) = default {
                    self.emit(&format!(
                        "{rt}::support::echo(&mut out, {}, false, false);",
                        literal(default)
                    ));
                }
            }
            Node::Include(name) => self.include(name)?,
            // A push renders in its stack, which `@stack` prints.
            Node::Push { .. } => {}
            Node::Stack(name) => {
                if let Some(bodies) = self.stacks.get(name).cloned() {
                    if self.stack_depth >= MAX_DEPTH {
                        return Err(self.error(format!("@stack('{name}') refers to itself")));
                    }
                    self.stack_depth += 1;
                    for body in bodies {
                        self.nodes(&body)?;
                    }
                    self.stack_depth -= 1;
                }
            }
            Node::Parent => {
                if let Some((name, index)) = self.section_stack.last().cloned() {
                    self.render_section(&name, index + 1)?;
                }
            }
            Node::HasSection {
                name,
                present,
                body,
            } => {
                if self.is_defined(name) == *present {
                    self.nodes(body)?;
                }
            }
            Node::Dump { expr, halt } => {
                let place = self.place(expr)?;
                let path = literal(&expr.path.join("."));
                self.emit("if ::core::cfg!(debug_assertions) {");
                self.emit(&format!(
                    "{rt}::support::dump(&mut out, {path}, &{place})?;"
                ));
                if *halt {
                    let label = self.fragment.clone();
                    self.emit(&format!("break {label};"));
                }
                self.emit("}");
            }
        }
        Ok(())
    }

    fn if_block(
        &mut self,
        branches: &[Branch],
        otherwise: Option<&[Node]>,
    ) -> Result<(), CompileError> {
        // Every condition's trait is emitted before the chain, since an `else if`
        // cannot hold an item.
        let mut checks = Vec::new();
        for Branch { condition, .. } in branches {
            checks.push(self.site(Site::Condition, condition)?);
        }
        for (index, (Branch { body, .. }, (func, place))) in branches.iter().zip(checks).enumerate()
        {
            let keyword = if index == 0 { "if" } else { "} else if" };
            self.emit(&format!("{keyword} {func}(&{place}) {{"));
            self.nodes(body)?;
        }
        if let Some(otherwise) = otherwise {
            self.emit("} else {");
            self.nodes(otherwise)?;
        }
        self.emit("}");
        Ok(())
    }

    fn is_defined(&self, name: &str) -> bool {
        self.sections.get(name).is_some_and(|defs| !defs.is_empty())
    }

    /// Renders definition `index` of section `name`. Inside it, `@parent`
    /// renders definition `index + 1`. Nothing renders when that does not exist.
    fn render_section(&mut self, name: &str, index: usize) -> Result<(), CompileError> {
        let Some(definition) = self
            .sections
            .get(name)
            .and_then(|defs| defs.get(index))
            .cloned()
        else {
            return Ok(());
        };
        if self.section_stack.len() >= MAX_DEPTH {
            return Err(self.error(format!("@section('{name}') refers to itself")));
        }

        self.section_stack.push((name.to_owned(), index));
        let result = self.fragment(&definition.template, |gen| gen.nodes(&definition.nodes));
        self.section_stack.pop();
        result
    }

    fn include(&mut self, name: &str) -> Result<(), CompileError> {
        if self.include_depth >= MAX_DEPTH {
            return Err(self.error(format!("@include chain at `{name}` is too deep (a cycle?)")));
        }
        let template = self.load(name)?;
        self.include_depth += 1;
        let result = self.fragment(name, |gen| gen.nodes(&template.nodes));
        self.include_depth -= 1;
        result
    }

    /// A name such as `ignite::resources/css/app.css` must use a namespace the
    /// app declared. Otherwise the view would fail at run time, so the build
    /// fails instead.
    fn check_asset_namespace(&self, name: &str) -> Result<(), CompileError> {
        let Some((namespace, _)) = name.split_once("::") else {
            return Ok(());
        };
        if self
            .options
            .asset_namespaces
            .iter()
            .any(|declared| declared == namespace)
        {
            return Ok(());
        }
        Err(self.error(format!(
            "asset namespace `{namespace}` is not declared: add `.asset_namespace(\"{namespace}\")` to the views in build.rs"
        )))
    }

    fn not_built(&self, path: &str) -> String {
        format!(
            "{rt}::RenderError::AssetNotBuilt {{ template: {template}.to_owned(), path: {path}.to_owned() }}",
            rt = self.options.runtime_path,
            template = literal(&self.template),
            path = literal(path),
        )
    }

    /// The Rust place for a path such as `post.title`. A loop variable shadows
    /// the context, as it does in the interpreter.
    fn place(&self, expr: &Expr) -> Result<String, CompileError> {
        let (root, rest) = expr
            .path
            .split_first()
            .ok_or_else(|| self.error("an empty path"))?;

        let mut place = if self.loops.iter().any(|item| item == root) {
            format!("v_{root}")
        } else {
            format!("ctx.{}", self.field(root)?)
        };
        // Each `.field` reads a field of the value before it, which may be null.
        // The probe returns that value, or a RenderError::NullValue that names
        // the whole path, which is what the interpreter reports.
        let rt = &self.options.runtime_path;
        let whole = literal(&expr.path.join("."));
        let template = literal(&self.template);
        for segment in rest {
            let field = self.field(segment)?;
            place = format!(
                "({rt}::support::Probe(&{place}).resolve().map_err(|_: {rt}::support::Null| \
                 {rt}::RenderError::NullValue {{ template: {template}.to_owned(), path: {whole}.to_owned() }})?).{field}"
            );
        }
        Ok(place)
    }

    fn field(&self, name: &str) -> Result<String, CompileError> {
        match name {
            "self" | "Self" | "super" | "crate" | "_" => Err(self.error(format!(
                "`{name}` cannot be a field name in a compiled view"
            ))),
            _ if KEYWORDS.contains(&name) => Ok(format!("r#{name}")),
            _ => Ok(name.to_owned()),
        }
    }
}

/// The top-level `@section`s of a template. A section anywhere else is an
/// error. Mirrors `collect_sections` in the interpreter.
fn collect_sections(
    name: &str,
    nodes: &[Node],
) -> Result<HashMap<String, Definition>, CompileError> {
    let nested = |section: &str| CompileError {
        template: name.to_owned(),
        message: format!("@section('{section}') must be at the top level of the template"),
    };

    let mut sections = HashMap::new();
    for node in nodes {
        match node {
            Node::Section {
                name: section,
                body,
                ..
            } => {
                if let Some(inner) = find_section(body) {
                    return Err(nested(inner));
                }
                sections.insert(
                    section.clone(),
                    Definition {
                        template: name.to_owned(),
                        nodes: body.clone(),
                    },
                );
            }
            other => {
                if let Some(inner) = find_section(std::slice::from_ref(other)) {
                    return Err(nested(inner));
                }
            }
        }
    }
    Ok(sections)
}

/// The name of the first `@section` anywhere inside `nodes`.
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

/// A Rust string literal for `text`.
fn literal(text: &str) -> String {
    format!("{text:?}")
}
