//! `PropelView::render` reads views from disk in every build, release included.
//! Run with `cargo test --release` to cover the release path.

use std::collections::HashMap;

use propel_rocket::{register_namespace, PropelView};
use rocket::http::Status;
use rocket::local::blocking::Client;
use rocket::routes;

type Context = HashMap<&'static str, &'static str>;

#[rocket::get("/greet")]
fn greet() -> PropelView {
    PropelView::render("rt_release::greet", &Context::from([("name", "Ada")]))
}

#[rocket::get("/missing")]
fn missing() -> PropelView {
    PropelView::render("rt_release::nope", &Context::new())
}

#[test]
fn renders_from_a_registered_namespace_and_sees_edits() {
    let root =
        std::env::temp_dir().join(format!("propel-rocket-rt-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("greet.propel"), "Hi {{ $name }}").unwrap();
    register_namespace("rt_release", &root);

    let client = Client::tracked(rocket::build().mount("/", routes![greet, missing])).unwrap();

    let response = client.get("/greet").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().as_deref(), Some("Hi Ada"));

    // Edit the file (different length): the next request sees it.
    std::fs::write(root.join("greet.propel"), "Hello again, {{ $name }}!").unwrap();
    let response = client.get("/greet").dispatch();
    assert_eq!(response.into_string().as_deref(), Some("Hello again, Ada!"));

    // A missing view is a render error (a 500 response), not a panic.
    let response = client.get("/missing").dispatch();
    assert_eq!(response.status(), Status::InternalServerError);

    let _ = std::fs::remove_dir_all(&root);
}
