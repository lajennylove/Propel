use propel_macros::PropelContext;
use serde::Serialize;

#[derive(Serialize, PropelContext)]
#[propel(root = "tests/ui/views", views("missing_field"))]
pub struct Ctx {
    pub title: String,
}

fn main() {}
