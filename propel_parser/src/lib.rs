//! Lexer, parser, and AST for the Propel templating language.
//!
//! Parsing is the first stage of the Propel pipeline: source template text
//! becomes an AST, which `propel_compiler` turns into Rust code.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {}
}
