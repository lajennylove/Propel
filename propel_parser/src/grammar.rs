//! The Pest parser generated from `propel.pest`.

use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "propel.pest"]
pub struct PropelParser;
