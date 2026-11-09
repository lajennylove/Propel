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
    /// `@section('name') ... @endsection`, or `@section ... @show` when `show` is set,
    /// which also renders the section where it stands.
    Section {
        name: String,
        body: Vec<Node>,
        show: bool,
    },
    /// `@yield('name')`.
    /// `@yield('name')`, or `@yield('name', 'default')`, which prints the default
    /// when no section fills the name.
    Yield {
        name: String,
        default: Option<String>,
    },
    /// `@include('name')`: renders another view here, with the same data.
    Include(String),
    /// `@push('name') ... @endpush`, or `@prepend` when `prepend` is set. It renders
    /// its body in the matching `@stack`, not where it stands.
    Push {
        name: String,
        body: Vec<Node>,
        prepend: bool,
    },
    /// `@stack('name')`: renders everything pushed to `name`.
    Stack(String),
    /// `@parent`: inside a section, the content of the same section one level up.
    Parent,
    /// `@hasSection('name') ... @endif`, or `@sectionMissing` when `present` is false.
    HasSection {
        name: String,
        present: bool,
        body: Vec<Node>,
    },
    /// `{{ asset('path') }}`: the URL of a built file, or of the dev server.
    Asset {
        path: String,
        raw: bool,
        url_attribute: bool,
    },
    /// `@vite(['entry', ...])`: the tags that load the entries.
    Vite { entries: Vec<String> },
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
///
/// `line` and `column` are where the path starts in the template, both from 1.
/// They are for error messages. Two paths are equal when their names are equal,
/// whatever their positions.
#[derive(Debug, Clone, Default)]
pub struct Expr {
    pub path: Vec<String>,
    pub line: usize,
    pub column: usize,
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
