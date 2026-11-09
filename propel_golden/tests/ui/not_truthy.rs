use propel_macros::views;

pub struct Post {
    pub title: String,
}

pub struct Ctx {
    pub post: Post,
}

views!(root = "tests/ui/views", view = "not_truthy", context = crate::Ctx);

fn main() {}
