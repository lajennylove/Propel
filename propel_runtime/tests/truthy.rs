//! Truthiness in `@if`, as PHP and Laravel: every object is true, including an
//! empty one, and only an empty list is false. The rule is the same in the
//! dev renderer (JSON) and in compiled views (`support::Truthy`).

use std::collections::HashMap;

use propel_runtime::render;
use propel_runtime::support::Truthy;
use serde::Serialize;

mod memory {
    use std::collections::HashMap;
    use std::io;

    use propel_runtime::Loader;

    pub struct Memory(pub HashMap<&'static str, &'static str>);

    impl Loader for Memory {
        fn source(&self, name: &str) -> io::Result<String> {
            self.0
                .get(name)
                .map(|source| source.to_string())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name.to_owned()))
        }
    }
}

fn page(source: &'static str) -> memory::Memory {
    memory::Memory(HashMap::from([("page", source)]))
}

// The derive lives in propel_macros, which depends on this crate, so the
// test writes the impl that the derive would write. The golden crate covers
// the derive itself.
#[derive(Serialize)]
struct Empty {}

impl Truthy for Empty {
    fn truthy(&self) -> bool {
        true
    }
}

#[derive(Serialize)]
struct Context {
    empty: Empty,
    empty_map: HashMap<String, String>,
    maybe_map: Option<HashMap<String, String>>,
    empty_list: Vec<u8>,
}

fn context() -> Context {
    Context {
        empty: Empty {},
        empty_map: HashMap::new(),
        maybe_map: None,
        empty_list: Vec::new(),
    }
}

#[test]
fn an_object_with_no_fields_is_true_in_the_dev_renderer() {
    let html = render(&page("@if ($empty)yes@else no@endif"), "page", &context()).unwrap();

    assert_eq!(html, "yes");
}

#[test]
fn an_empty_map_is_true_so_an_optional_map_is_the_way_to_make_it_false() {
    let c = context();
    let always = render(&page("@if ($empty_map)yes@else no@endif"), "page", &c).unwrap();
    let optional = render(&page("@if ($maybe_map)yes@else no@endif"), "page", &c).unwrap();

    assert_eq!(always, "yes");
    assert_eq!(optional, " no");
}

#[test]
fn an_empty_list_is_false() {
    let html = render(
        &page("@if ($empty_list)yes@else no@endif"),
        "page",
        &context(),
    )
    .unwrap();

    assert_eq!(html, " no");
}

#[test]
fn compiled_truthy_matches_the_dev_rule() {
    assert!(Empty {}.truthy());
    assert!(HashMap::<String, String>::new().truthy());
    assert!(!Vec::<u8>::new().truthy());
    assert!(!None::<HashMap<String, String>>.truthy());
}
