//! The template AST produced by the parser.

/// A parsed template: the top-level nodes of one `.propel` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// Literal template text, copied to the output unchanged.
    Text(String),
    /// `{{ expr }}` (escaped) or `{!! expr !!}` (raw). `url_attribute` is set
    /// when the echo sits in an `href`, `src` or similar value; see `markup`.
    Echo {
        expr: Expr,
        raw: bool,
        url_attribute: bool,
    },
    /// `@if`, `@elseif`, `@else`, `@endif`.
    If {
        branches: Vec<Branch>,
        otherwise: Option<Vec<Node>>,
    },
    /// `@foreach` and `@forelse`. `empty` is the `@empty` body of `@forelse`.
    Loop {
        item: String,
        collection: Expr,
        body: Vec<Node>,
        empty: Option<Vec<Node>>,
    },
    /// `@extends('name')`.
    Extends(String),
    /// `@section('name') ... @endsection`.
    Section { name: String, body: Vec<Node> },
    /// `@yield('name')`.
    Yield(String),
    /// `@dump($expr)`, or `@dd($expr)` when `halt` is set.
    Dump { expr: Expr, halt: bool },
}

/// One `@if` or `@elseif` arm.
#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    pub condition: Expr,
    pub body: Vec<Node>,
}

/// A variable path such as `title` or `post.title`, without the leading `$`.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub path: Vec<String>,
}
