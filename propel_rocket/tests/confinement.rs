//! A confined namespace registered with `register_confined_namespace` cannot
//! read outside its root, and can extend its parent.

use std::collections::HashMap;

use propel_rocket::{register_confined_namespace, PropelView};
use rocket::http::Status;
use rocket::local::blocking::Client;
use rocket::routes;

type Context = HashMap<&'static str, &'static str>;

#[rocket::get("/escape")]
fn escape() -> PropelView {
    PropelView::render("conf_child::escape", &Context::new())
}

#[rocket::get("/page")]
fn page() -> PropelView {
    PropelView::render("conf_child::page", &Context::new())
}

#[test]
fn confined_themes_stay_inside_their_root_and_parent() {
    let base = std::env::temp_dir().join(format!("propel-rocket-confined-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (parent, child) = (base.join("parent"), base.join("child"));
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(parent.join("layout.propel"), "<b>@yield('c')</b>").unwrap();
    std::fs::write(base.join("secrets.propel"), "SECRET").unwrap();
    std::fs::write(
        child.join("page.propel"),
        "@extends('conf_parent::layout')@section('c')ok@endsection",
    )
    .unwrap();
    std::fs::write(child.join("escape.propel"), "@include('../../secrets')").unwrap();
    register_confined_namespace("conf_parent", &parent, None);
    register_confined_namespace("conf_child", &child, Some("conf_parent"));

    let client = Client::tracked(rocket::build().mount("/", routes![escape, page])).unwrap();

    let response = client.get("/page").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().as_deref(), Some("<b>ok</b>"));

    let response = client.get("/escape").dispatch();
    assert_eq!(response.status(), Status::InternalServerError);

    let _ = std::fs::remove_dir_all(&base);
}
