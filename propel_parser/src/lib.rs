//! Lexer, parser, and AST for the Propel templating language.
//!
//! Parsing is the first stage of the Propel pipeline: template text becomes
//! an AST. `propel_runtime` renders that AST in dev builds, and
//! `propel_compiler` turns it into Rust.

mod ast;
mod grammar;

use std::fmt;

use pest::iterators::Pair;
use pest::Parser;

pub use ast::*;
use grammar::{PropelParser, Rule};

/// A template that failed to parse. Display shows the position of the error.
#[derive(Debug)]
pub struct ParseError(pest::error::Error<Rule>);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for ParseError {}

/// Parses one template.
pub fn parse(source: &str) -> Result<Template, ParseError> {
    let file = PropelParser::parse(Rule::file, source)
        .map_err(ParseError)?
        .next()
        .expect("the `file` rule always produces one pair");

    let nodes = file
        .into_inner()
        .filter(|pair| pair.as_rule() != Rule::EOI)
        .map(node)
        .collect();

    Ok(Template { nodes })
}

fn node(pair: Pair<'_, Rule>) -> Node {
    match pair.as_rule() {
        Rule::text => Node::Text(pair.as_str().to_owned()),
        Rule::echo => {
            let inner = child(pair);
            Node::Echo {
                raw: inner.as_rule() == Rule::raw_echo,
                expr: expr(child(inner)),
            }
        }
        Rule::if_block => if_block(pair),
        Rule::foreach_block => {
            let (item, collection) = loop_head(child(pair.clone()));
            let body = pair.into_inner().nth(1).map(body).unwrap_or_default();
            Node::Loop {
                item,
                collection,
                body,
                empty: None,
            }
        }
        Rule::forelse_block => forelse_block(pair),
        Rule::extends => Node::Extends(string(child(pair))),
        Rule::section => {
            let mut inner = pair.into_inner();
            let name = string(inner.next().expect("section name"));
            let body = body(inner.next().expect("section body"));
            Node::Section { name, body }
        }
        Rule::yield_ => Node::Yield(string(child(pair))),
        rule => unreachable!("grammar produced unexpected node: {rule:?}"),
    }
}

fn if_block(pair: Pair<'_, Rule>) -> Node {
    let mut inner = pair.into_inner();
    let condition = expr(inner.next().expect("if condition"));
    let first_body = body(inner.next().expect("if body"));

    let mut branches = vec![Branch {
        condition,
        body: first_body,
    }];
    let mut otherwise = None;

    for pair in inner {
        match pair.as_rule() {
            Rule::elseif_branch => {
                let mut inner = pair.into_inner();
                let condition = expr(inner.next().expect("elseif condition"));
                let body = body(inner.next().expect("elseif body"));
                branches.push(Branch { condition, body });
            }
            Rule::else_branch => otherwise = Some(body(child(pair))),
            rule => unreachable!("grammar produced unexpected if part: {rule:?}"),
        }
    }

    Node::If {
        branches,
        otherwise,
    }
}

fn forelse_block(pair: Pair<'_, Rule>) -> Node {
    let mut inner = pair.into_inner();
    let (item, collection) = loop_head(inner.next().expect("loop head"));
    let loop_body = body(inner.next().expect("loop body"));
    // An absent `@empty` renders nothing, so it is an empty list here.
    let empty = Some(inner.next().map(child).map(body).unwrap_or_default());

    Node::Loop {
        item,
        collection,
        body: loop_body,
        empty,
    }
}

fn loop_head(pair: Pair<'_, Rule>) -> (String, Expr) {
    let mut inner = pair.into_inner();
    let item = inner.next().expect("loop item").as_str().to_owned();
    let collection = expr(inner.next().expect("loop collection"));
    (item, collection)
}

fn body(pair: Pair<'_, Rule>) -> Vec<Node> {
    pair.into_inner().map(node).collect()
}

fn expr(pair: Pair<'_, Rule>) -> Expr {
    let path = pair
        .as_str()
        .trim_start_matches('$')
        .split('.')
        .map(str::to_owned)
        .collect();

    Expr { path }
}

/// Strips the quotes from a `string` pair.
fn string(pair: Pair<'_, Rule>) -> String {
    let text = pair.as_str();
    text[1..text.len() - 1].to_owned()
}

fn child(pair: Pair<'_, Rule>) -> Pair<'_, Rule> {
    pair.into_inner()
        .next()
        .expect("grammar guarantees a child pair")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(source: &str) -> Vec<Node> {
        parse(source).expect("valid template").nodes
    }

    #[test]
    fn text_and_echoes() {
        assert_eq!(
            nodes("Hi {{ $name }}, {!! $html !!}!"),
            vec![
                Node::Text("Hi ".into()),
                Node::Echo {
                    expr: Expr {
                        path: vec!["name".into()]
                    },
                    raw: false,
                },
                Node::Text(", ".into()),
                Node::Echo {
                    expr: Expr {
                        path: vec!["html".into()]
                    },
                    raw: true,
                },
                Node::Text("!".into()),
            ]
        );
    }

    #[test]
    fn dotted_paths() {
        assert_eq!(
            nodes("{{ post.author.name }}"),
            vec![Node::Echo {
                expr: Expr {
                    path: vec!["post".into(), "author".into(), "name".into()],
                },
                raw: false,
            }]
        );
    }

    #[test]
    fn lone_at_is_text() {
        assert_eq!(nodes("me@ x"), vec![Node::Text("me@ x".into())]);
    }

    #[test]
    fn comments_are_dropped() {
        assert_eq!(
            nodes("a{{-- hidden --}}b"),
            vec![Node::Text("a".into()), Node::Text("b".into()),]
        );
    }

    #[test]
    fn if_elseif_else() {
        let parsed = nodes("@if (a) A @elseif (b) B @else C @endif");
        let Node::If {
            branches,
            otherwise,
        } = &parsed[0]
        else {
            panic!("expected an if, got {parsed:?}");
        };

        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].condition.path, ["a"]);
        assert_eq!(branches[1].condition.path, ["b"]);
        assert_eq!(otherwise.as_deref(), Some(&[Node::Text(" C ".into())][..]));
    }

    #[test]
    fn foreach_and_forelse() {
        let parsed = nodes("@foreach (p in ps) x @endforeach");
        assert!(matches!(
            &parsed[0],
            Node::Loop { item, empty: None, .. } if item == "p"
        ));

        let parsed = nodes("@forelse (p in ps) x @empty none @endforelse");
        let Node::Loop { empty, .. } = &parsed[0] else {
            panic!("expected a loop");
        };
        assert_eq!(empty.as_deref(), Some(&[Node::Text(" none ".into())][..]));
    }

    #[test]
    fn layout_directives() {
        assert_eq!(
            nodes("@extends('layouts.app')"),
            vec![Node::Extends("layouts.app".into()),]
        );

        let parsed = nodes("@section(\"title\") T @endsection @yield('title')");
        assert!(matches!(&parsed[0], Node::Section { name, .. } if name == "title"));
        assert_eq!(parsed[2], Node::Yield("title".into()));
    }

    #[test]
    fn unclosed_if_is_an_error() {
        assert!(parse("@if (a) text").is_err());
    }

    #[test]
    fn stray_end_directive_is_an_error() {
        assert!(parse("text @endif").is_err());
    }
}
