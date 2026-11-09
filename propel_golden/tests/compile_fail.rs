//! Compile-fail cases for the checks that run while the app compiles. Each
//! file in `tests/ui` must fail to build, and its message is kept in the
//! matching `.stderr` file. Run with `TRYBUILD=overwrite` to write new ones,
//! then read them before committing.

#[test]
fn compile_errors_name_the_template_position() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
