use propel_macros::views;

pub struct Ctx {
    pub title: String,
    pub posts: Vec<u8>,
}

views!(root = "tests/ui/views", view = "wrong_type", context = crate::Ctx);

fn main() {}
