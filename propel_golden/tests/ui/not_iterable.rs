use propel_macros::views;

pub struct Ctx {
    pub title: String,
}

views!(root = "tests/ui/views", view = "not_iterable", context = crate::Ctx);

fn main() {}
