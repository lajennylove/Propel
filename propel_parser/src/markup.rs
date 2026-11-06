//! Tracks where each `{{ }}` sits in the HTML around it, so the renderer knows
//! when a value is a URL.
//!
//! This is a small state machine over the template's text, read in document
//! order. It tracks tags and attribute values, not a full HTML parse. It is
//! enough for `href="{{ $url }}"`, `src={{ $url }}` and similar, which is the
//! case that needs a different escape.
//!
//! Limits, documented in `propel/README.md`: the state follows the template
//! text in order, so a branch of an `@if` that opens an attribute and another
//! branch that closes it is not handled. Sections are read as their own
//! fragments and start in text state.

use crate::ast::Node;

/// Attribute names whose values are URLs.
const URL_ATTRIBUTES: &[&str] = &["href", "src", "action", "formaction", "xlink:href"];

/// What the tokenizer is inside, at the end of the text read so far.
#[derive(Debug, Clone, PartialEq)]
enum State {
    /// Ordinary text: element content or a comment.
    Text,
    /// Inside a tag, between attribute values.
    Tag {
        /// The attribute name being read, or the last one read.
        name: String,
        /// The name is complete (a space followed it), so the next name char starts a new one.
        name_done: bool,
        /// An `=` was read, so the next non-space character starts a value.
        expect_value: bool,
    },
    /// Inside an attribute value.
    Value {
        url: bool,
        /// The quote that closes the value, or `None` for an unquoted value.
        quote: Option<char>,
    },
}

impl State {
    fn tag() -> State {
        State::Tag {
            name: String::new(),
            name_done: false,
            expect_value: false,
        }
    }

    fn is_url_value(&self) -> bool {
        matches!(self, State::Value { url: true, .. })
    }

    fn advance(&mut self, text: &str) {
        for c in text.chars() {
            *self = self.next(c);
        }
    }

    fn next(&self, c: char) -> State {
        match self {
            State::Text => match c {
                '<' => State::tag(),
                _ => State::Text,
            },
            State::Tag {
                name,
                name_done,
                expect_value,
            } => {
                if *expect_value {
                    return match c {
                        c if c.is_whitespace() => self.clone(),
                        '"' | '\'' => State::Value {
                            url: is_url_attribute(name),
                            quote: Some(c),
                        },
                        '>' => State::Text,
                        _ => State::Value {
                            url: is_url_attribute(name),
                            quote: None,
                        },
                    };
                }

                match c {
                    '>' => State::Text,
                    '=' => State::Tag {
                        name: name.clone(),
                        name_done: true,
                        expect_value: true,
                    },
                    c if c.is_whitespace() => State::Tag {
                        name: name.clone(),
                        name_done: true,
                        expect_value: false,
                    },
                    c => {
                        let mut name = if *name_done {
                            String::new()
                        } else {
                            name.clone()
                        };
                        name.push(c.to_ascii_lowercase());
                        State::Tag {
                            name,
                            name_done: false,
                            expect_value: false,
                        }
                    }
                }
            }
            State::Value { quote, .. } => match quote {
                Some(q) if c == *q => State::tag(),
                Some(_) => self.clone(),
                None if c == '>' => State::Text,
                None if c.is_whitespace() => State::tag(),
                None => self.clone(),
            },
        }
    }
}

fn is_url_attribute(name: &str) -> bool {
    URL_ATTRIBUTES.contains(&name)
}

/// Marks every echo that sits in a URL attribute value.
pub(crate) fn annotate(nodes: &mut [Node]) {
    let mut state = State::Text;
    mark(nodes, &mut state);
}

fn mark(nodes: &mut [Node], state: &mut State) {
    for node in nodes {
        match node {
            Node::Text(text) => state.advance(text),
            Node::Echo { url_attribute, .. } => {
                // An unquoted value can start with an echo: `src={{ $url }}`.
                // The echo opens the value, so it is read as one.
                if let State::Tag {
                    name,
                    expect_value: true,
                    ..
                } = state
                {
                    let url = is_url_attribute(name);
                    *state = State::Value { url, quote: None };
                }
                *url_attribute = state.is_url_value();
            }
            Node::If {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    mark(&mut branch.body, state);
                }
                if let Some(otherwise) = otherwise {
                    mark(otherwise, state);
                }
            }
            Node::Loop { body, empty, .. } => {
                mark(body, state);
                if let Some(empty) = empty {
                    mark(empty, state);
                }
            }
            Node::Section { body, .. } => {
                let mut section_state = State::Text;
                mark(body, &mut section_state);
            }
            Node::Extends(_) | Node::Yield(_) | Node::Dump { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// Which echoes are URL echoes, in document order.
    fn url_echoes(source: &str) -> Vec<bool> {
        fn collect(nodes: &[Node], out: &mut Vec<bool>) {
            for node in nodes {
                match node {
                    Node::Echo { url_attribute, .. } => out.push(*url_attribute),
                    Node::If {
                        branches,
                        otherwise,
                    } => {
                        for branch in branches {
                            collect(&branch.body, out);
                        }
                        if let Some(otherwise) = otherwise {
                            collect(otherwise, out);
                        }
                    }
                    Node::Loop { body, empty, .. } => {
                        collect(body, out);
                        if let Some(empty) = empty {
                            collect(empty, out);
                        }
                    }
                    Node::Section { body, .. } => collect(body, out),
                    _ => {}
                }
            }
        }

        let mut out = Vec::new();
        collect(&parse(source).unwrap().nodes, &mut out);
        out
    }

    #[test]
    fn quoted_href_is_a_url() {
        assert_eq!(url_echoes(r#"<a href="{{ $u }}">"#), [true]);
        assert_eq!(url_echoes("<a href='{{ $u }}'>"), [true]);
    }

    #[test]
    fn unquoted_src_is_a_url() {
        assert_eq!(url_echoes("<img src={{ $u }}>"), [true]);
    }

    #[test]
    fn element_text_is_not_a_url() {
        assert_eq!(url_echoes("<p>{{ $u }}</p>"), [false]);
    }

    #[test]
    fn other_attributes_are_not_urls() {
        assert_eq!(url_echoes(r#"<a title="{{ $u }}">"#), [false]);
    }

    #[test]
    fn state_resets_after_the_value_closes() {
        assert_eq!(
            url_echoes(r#"<a href="/x" title="{{ $t }}">{{ $u }}</a>"#),
            [false, false]
        );
    }

    #[test]
    fn spaces_around_equals_are_handled() {
        assert_eq!(url_echoes(r#"<a href = "{{ $u }}">"#), [true]);
    }

    #[test]
    fn upper_case_names_are_handled() {
        assert_eq!(url_echoes(r#"<A HREF="{{ $u }}">"#), [true]);
    }
}
