//! Helpers for compiled views.
//!
//! `propel_compiler` generates Rust code that calls these functions and
//! traits. They share the escaping, URL and dump code with the dev renderer,
//! so the two renderers cannot disagree on those. The generated code refers to
//! this module by path, so an app does not call it directly.

use propel_parser::Expr;
use serde::Serialize;

use crate::RenderError;

/// A value as echo text. `None` echoes as nothing, and a value that has no
/// text form (a struct or a list) does not implement this trait, so using it
/// in `{{ }}` is a compile error.
pub trait Printable {
    fn text(&self) -> String;
}

impl<T: Printable + ?Sized> Printable for &T {
    fn text(&self) -> String {
        (**self).text()
    }
}

impl Printable for str {
    fn text(&self) -> String {
        self.to_owned()
    }
}

impl Printable for String {
    fn text(&self) -> String {
        self.clone()
    }
}

impl Printable for bool {
    fn text(&self) -> String {
        self.to_string()
    }
}

macro_rules! printable_integers {
    ($($ty:ty),*) => {$(
        impl Printable for $ty {
            fn text(&self) -> String {
                self.to_string()
            }
        }
    )*};
}
printable_integers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

/// Floats print as the dev renderer's JSON does: `1.0`, not `1`. A NaN or an
/// infinity is `null` in JSON, so it echoes as nothing.
macro_rules! printable_floats {
    ($($ty:ty),*) => {$(
        impl Printable for $ty {
            fn text(&self) -> String {
                if self.is_finite() {
                    format!("{self:?}")
                } else {
                    String::new()
                }
            }
        }
    )*};
}
printable_floats!(f32, f64);

impl<T: Printable> Printable for Option<T> {
    fn text(&self) -> String {
        self.as_ref().map(Printable::text).unwrap_or_default()
    }
}

/// Truthiness for `@if`. `false`, zero, an empty string or list, and `None`
/// are false. Every object is true, so a struct is true: `#[derive(Truthy)]`
/// gives that. A hand-written impl is not supported. The dev renderer reads
/// JSON and cannot run it, so the two renderers would differ. Test a field in
/// the template instead, for example `@if ($draft.body)`.
pub trait Truthy {
    fn truthy(&self) -> bool;
}

impl<T: Truthy + ?Sized> Truthy for &T {
    fn truthy(&self) -> bool {
        (**self).truthy()
    }
}

impl Truthy for bool {
    fn truthy(&self) -> bool {
        *self
    }
}

impl Truthy for str {
    fn truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl Truthy for String {
    fn truthy(&self) -> bool {
        !self.is_empty()
    }
}

macro_rules! truthy_integers {
    ($($ty:ty),*) => {$(
        impl Truthy for $ty {
            fn truthy(&self) -> bool {
                *self != 0
            }
        }
    )*};
}
truthy_integers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

/// NaN is `null` in JSON, which is false, so it is false here too.
macro_rules! truthy_floats {
    ($($ty:ty),*) => {$(
        impl Truthy for $ty {
            fn truthy(&self) -> bool {
                *self != 0.0 && !self.is_nan()
            }
        }
    )*};
}
truthy_floats!(f32, f64);

impl<T: Truthy> Truthy for Option<T> {
    fn truthy(&self) -> bool {
        self.as_ref().is_some_and(Truthy::truthy)
    }
}

impl<T> Truthy for Vec<T> {
    fn truthy(&self) -> bool {
        !self.is_empty()
    }
}

/// A map is an object, and every object is true (see the dev renderer). An
/// empty map is therefore true too. To make an empty map false, use
/// `Option<HashMap<..>>` and set it to `None`.
impl<K, V, S> Truthy for std::collections::HashMap<K, V, S> {
    fn truthy(&self) -> bool {
        true
    }
}

impl<K, V> Truthy for std::collections::BTreeMap<K, V> {
    fn truthy(&self) -> bool {
        true
    }
}

/// The items of a loop collection. A `None` list is empty.
pub trait Items {
    type Item;

    fn items(&self) -> &[Self::Item];
}

impl<T> Items for Vec<T> {
    type Item = T;

    fn items(&self) -> &[T] {
        self
    }
}

impl<T> Items for Option<Vec<T>> {
    type Item = T;

    fn items(&self) -> &[T] {
        self.as_deref().unwrap_or(&[])
    }
}

/// Writes an echoed value, the way `{{ }}` and `{!! !!}` do. A value in a URL
/// attribute with an unsafe scheme is replaced by `#`, even when it is raw.
pub fn echo(out: &mut String, text: &str, raw: bool, url_attribute: bool) {
    if url_attribute && !crate::is_safe_url(text) {
        out.push('#');
    } else if raw {
        out.push_str(text);
    } else {
        crate::escape_into(text, out);
    }
}

/// Writes an asset URL, escaped unless it is raw.
pub fn url(out: &mut String, url: &str, raw: bool) {
    if raw {
        out.push_str(url);
    } else {
        crate::escape_into(url, out);
    }
}

/// Writes the `@dump` output for `value`. `path` is the expression as written,
/// such as `post.title`. Only debug builds call this.
pub fn dump<T: Serialize + ?Sized>(
    out: &mut String,
    path: &str,
    value: &T,
) -> Result<(), RenderError> {
    let value = serde_json::to_value(value).map_err(RenderError::Context)?;
    let expr = Expr {
        path: path.split('.').map(str::to_owned).collect(),
        ..Expr::default()
    };
    out.push_str(&crate::dump_html(&expr, &value));
    Ok(())
}

/// The error of a probe: the value is null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Null;

/// The first step of a path whose value might be null. Generated code calls
/// `Probe(&value).resolve()` before each `.field` that follows an optional
/// value, and maps [`Null`] to `RenderError::NullValue`.
///
/// Two traits give the same method name for two cases. Rust tries the by-value
/// receiver first, so an `Option` takes [`NullableProbe`], and any other type
/// falls through to [`PlainProbe`]. Both traits must be in scope where the
/// method is called.
pub struct Probe<'a, X: ?Sized>(pub &'a X);

/// The value of an `Option`: `Err` when it is `None`.
pub trait NullableProbe<'a> {
    type Inner: ?Sized + 'a;

    fn resolve(self) -> Result<&'a Self::Inner, Null>;
}

impl<'a, T> NullableProbe<'a> for Probe<'a, Option<T>> {
    type Inner = T;

    fn resolve(self) -> Result<&'a T, Null> {
        self.0.as_ref().ok_or(Null)
    }
}

/// Any other value: it is its own value.
pub trait PlainProbe<'a> {
    type Inner: ?Sized + 'a;

    fn resolve(&self) -> Result<&'a Self::Inner, Null>;
}

impl<'a, X: ?Sized> PlainProbe<'a> for Probe<'a, X> {
    type Inner = X;

    fn resolve(&self) -> Result<&'a X, Null> {
        Ok(self.0)
    }
}
