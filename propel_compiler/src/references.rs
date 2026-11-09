//! Every context field a view's templates read, with its template position.
//!
//! The `PropelContext` derive uses this to check the context struct's fields
//! while the app compiles, so a missing field names the template and line. The
//! walk follows the same rules as code generation: the sections that render,
//! the loop variables in scope, and the partials that are included. A loop
//! variable shadows a field of the same name, as it does in the renderers.

use std::collections::HashMap;

use propel_parser::{Expr, Node, Template};

use crate::{CompileError, Templates};

/// How deep layouts and partials may nest, as in the code generator.
const MAX_DEPTH: usize = 32;

/// One read of a context field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The template the read is in, such as `posts.index`.
    pub template: String,
    /// The file of that template, for error messages.
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// The field that is read: the first segment of the path.
    pub root: String,
    /// The whole path, such as `post.title`.
    pub path: String,
}

/// Every context field read by `entry` and the templates it uses.
pub fn references(templates: &dyn Templates, entry: &str) -> Result<Vec<Reference>, CompileError> {
    let mut walker = Walker {
        templates,
        found: Vec::new(),
        template: entry.to_owned(),
        loops: Vec::new(),
        sections: HashMap::new(),
        section_stack: Vec::new(),
        depth: 0,
    };
    walker.view(entry)?;
    Ok(walker.found)
}

struct Walker<'a> {
    templates: &'a dyn Templates,
    found: Vec<Reference>,
    /// The template being walked, for references and errors.
    template: String,
    /// Loop variables in scope, innermost last.
    loops: Vec<String>,
    /// The definitions of each section, most derived first.
    sections: HashMap<String, Vec<(String, Vec<Node>)>>,
    /// The definitions being walked, innermost last, with their index.
    section_stack: Vec<(String, usize)>,
    depth: usize,
}

impl Walker<'_> {
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

    /// Walks a view the way the renderers do: the rendered layout chain, with
    /// the sections of the views below it, and every `@push` on the way.
    fn view(&mut self, entry: &str) -> Result<(), CompileError> {
        let mut chain: Vec<(String, Template)> = Vec::new();
        let mut name = entry.to_owned();
        loop {
            if chain.len() > MAX_DEPTH {
                return Err(self.error("@extends chain is too deep (a cycle?)"));
            }
            let template = self.load(&name)?;
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

        for (name, template) in &chain {
            for node in &template.nodes {
                if let Node::Section {
                    name: section,
                    body,
                    ..
                } = node
                {
                    self.sections
                        .entry(section.clone())
                        .or_default()
                        .push((name.clone(), body.clone()));
                }
            }
        }

        // Pushes render in their stack, wherever the stack is, so every push
        // in the chain is read, whether or not its stack is printed.
        for (name, template) in &chain {
            self.template = name.clone();
            self.pushes(&template.nodes, 0)?;
        }

        let (root_name, root) = chain.pop().expect("the chain has the entry view");
        self.template = root_name;
        self.nodes(&root.nodes)
    }

    /// Walks every `@push` body reachable from `nodes`, including partials.
    fn pushes(&mut self, nodes: &[Node], depth: usize) -> Result<(), CompileError> {
        for node in nodes {
            match node {
                Node::Push { body, .. } => {
                    self.nodes(body)?;
                    self.pushes(body, depth)?;
                }
                Node::Section { body, .. } | Node::HasSection { body, .. } => {
                    self.pushes(body, depth)?
                }
                Node::If {
                    branches,
                    otherwise,
                } => {
                    for branch in branches {
                        self.pushes(&branch.body, depth)?;
                    }
                    if let Some(otherwise) = otherwise {
                        self.pushes(otherwise, depth)?;
                    }
                }
                Node::Loop { body, empty, .. } => {
                    self.pushes(body, depth)?;
                    if let Some(empty) = empty {
                        self.pushes(empty, depth)?;
                    }
                }
                Node::Include(name) => {
                    if depth >= MAX_DEPTH {
                        return Err(self
                            .error(format!("@include chain at `{name}` is too deep (a cycle?)")));
                    }
                    let template = self.load(name)?;
                    self.pushes(&template.nodes, depth + 1)?;
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
        match node {
            Node::Echo { expr, .. } | Node::Dump { expr, .. } => self.read(expr),
            Node::If {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    self.read(&branch.condition)?;
                    self.nodes(&branch.body)?;
                }
                match otherwise {
                    Some(otherwise) => self.nodes(otherwise),
                    None => Ok(()),
                }
            }
            Node::Loop {
                item,
                collection,
                body,
                empty,
            } => {
                self.read(collection)?;
                self.loops.push(item.clone());
                let result = self.nodes(body);
                self.loops.pop();
                result?;
                match empty {
                    Some(empty) => self.nodes(empty),
                    None => Ok(()),
                }
            }
            // A section renders only where it is shown, or where a layout
            // yields it. Other definitions are read at their yield.
            Node::Section { name, show, .. } => {
                if *show {
                    self.render_section(name, 0)?;
                }
                Ok(())
            }
            Node::Yield { name, .. } => {
                if self.is_defined(name) {
                    self.render_section(name, 0)?;
                }
                Ok(())
            }
            Node::Parent => match self.section_stack.last().cloned() {
                Some((name, index)) => self.render_section(&name, index + 1),
                None => Ok(()),
            },
            Node::HasSection {
                name,
                present,
                body,
            } => {
                if self.is_defined(name) == *present {
                    self.nodes(body)?;
                }
                Ok(())
            }
            Node::Include(name) => {
                if self.depth >= MAX_DEPTH {
                    return Err(
                        self.error(format!("@include chain at `{name}` is too deep (a cycle?)"))
                    );
                }
                let template = self.load(name)?;
                let outer = std::mem::replace(&mut self.template, name.clone());
                self.depth += 1;
                let result = self.nodes(&template.nodes);
                self.depth -= 1;
                self.template = outer;
                result
            }
            // Pushes were read up front. Text, assets, `@vite`, `@stack` and
            // `@extends` read no context field.
            Node::Text(_)
            | Node::Asset { .. }
            | Node::Vite { .. }
            | Node::Extends(_)
            | Node::Stack(_)
            | Node::Push { .. } => Ok(()),
        }
    }

    fn is_defined(&self, name: &str) -> bool {
        self.sections.get(name).is_some_and(|defs| !defs.is_empty())
    }

    fn render_section(&mut self, name: &str, index: usize) -> Result<(), CompileError> {
        let Some((template, nodes)) = self
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

        let outer = std::mem::replace(&mut self.template, template);
        self.section_stack.push((name.to_owned(), index));
        let result = self.nodes(&nodes);
        self.section_stack.pop();
        self.template = outer;
        result
    }

    /// Records a read of the path `expr`, unless its first segment is a loop variable.
    fn read(&mut self, expr: &Expr) -> Result<(), CompileError> {
        let Some(root) = expr.path.first() else {
            return Ok(());
        };
        if !self.loops.contains(root) {
            self.found.push(Reference {
                template: self.template.clone(),
                file: self.templates.path(&self.template),
                line: expr.line,
                column: expr.column,
                root: root.clone(),
                path: expr.path.join("."),
            });
        }
        Ok(())
    }
}
