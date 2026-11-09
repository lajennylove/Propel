fn main() {
    propel_compiler::build::Views::new("golden/views")
        .namespace("ui", "golden/packages/ui")
        .view("echo", "crate::Basics")
        .view("conditions", "crate::Basics")
        .view("loops", "crate::Basics")
        .view("escaping", "crate::Basics")
        .view("layout", "crate::Basics")
        .view("sections", "crate::Basics")
        .view("stacks", "crate::Basics")
        .view("includes", "crate::Basics")
        .view("namespaced", "crate::Basics")
        .view("assets", "crate::Basics")
        .view("dump", "crate::Basics")
        .view("halt", "crate::Basics")
        .build();
}
