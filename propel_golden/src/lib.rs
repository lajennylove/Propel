//! The contexts and compiled views of the golden tests. See `tests/golden.rs`.

pub use propel_macros::PropelContext;
pub use propel_macros::Truthy;
use std::collections::HashMap;

use serde::Serialize;

/// A post. A struct has no text form, so it can be used in a loop and in a
/// path, but not echoed. It is truthy when present, as an `Option` that is set is.
#[derive(Serialize, Truthy)]
pub struct Post {
    pub title: String,
    pub url: String,
    pub published: Option<String>,
}

/// A draft. A rule that depends on its text is a field test in the template,
/// `@if ($draft.body)`, which both renderers run the same way.
#[derive(Serialize)]
pub struct Draft {
    pub body: String,
}

/// A struct with no fields. It serializes to `{}`, which is an object, so it
/// is true in both renderers.
#[derive(Serialize, Truthy)]
pub struct Marker {}

/// The context shared by the golden views. The fields cover every kind of
/// value the templates use: text, numbers, booleans, options, lists and structs.
/// Every view that reads `Basics`. The derive checks each field a view reads
/// while the crate compiles.
#[derive(Serialize, PropelContext)]
#[propel(
    root = "golden/views",
    namespace("ui", "golden/packages/ui"),
    views(
        "echo",
        "conditions",
        "loops",
        "escaping",
        "layout",
        "sections",
        "stacks",
        "includes",
        "namespaced",
        "assets",
        "dump",
        "halt"
    )
)]
pub struct Basics {
    pub title: String,
    pub raw_html: String,
    pub url: String,
    pub unsafe_url: String,
    pub count: u32,
    pub ratio: f64,
    pub flag: bool,
    pub nothing: Option<String>,
    pub subtitle: Option<String>,
    pub empty: String,
    pub tags: Vec<String>,
    pub maybe_tags: Option<Vec<String>>,
    pub posts: Vec<Post>,
    pub archived: Option<Vec<Post>>,
    pub latest: Option<Post>,
    pub draft: Draft,
    pub marker: Option<Marker>,
    pub map: Option<HashMap<String, String>>,
}

impl Basics {
    /// Every field has a value, so every branch can be seen.
    pub fn full() -> Self {
        let post = |title: &str| Post {
            title: title.into(),
            url: format!("/posts/{}", title.to_lowercase().replace(' ', "-")),
            published: Some("2026-11-05".into()),
        };
        Self {
            title: "Hello <Propel> & \"friends\"".into(),
            raw_html: "<b>bold</b> & \"quoted\"".into(),
            url: "https://example.com/?a=1&b=2".into(),
            unsafe_url: "javascript:alert(1)".into(),
            count: 3,
            ratio: 2.0,
            flag: true,
            nothing: None,
            subtitle: Some("Sub".into()),
            empty: String::new(),
            tags: vec!["rust".into(), "web".into()],
            maybe_tags: None,
            posts: vec![post("First post"), post("Second post")],
            archived: Some(vec![post("Old post")]),
            latest: Some(post("First post")),
            draft: Draft {
                body: "unfinished".into(),
            },
            marker: Some(Marker {}),
            map: Some(HashMap::new()),
        }
    }

    /// The empty states: no items, no value, and false flags.
    pub fn empty() -> Self {
        Self {
            title: "Nothing yet".into(),
            raw_html: String::new(),
            url: "/".into(),
            unsafe_url: String::new(),
            count: 0,
            ratio: 0.0,
            flag: false,
            nothing: None,
            subtitle: None,
            empty: String::new(),
            tags: Vec::new(),
            maybe_tags: None,
            posts: Vec::new(),
            archived: None,
            latest: None,
            draft: Draft {
                body: String::new(),
            },
            marker: None,
            map: None,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/propel_views.rs"));
