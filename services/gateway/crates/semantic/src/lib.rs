//! The semantic layer (docs/adr/0006): the expression language that metrics and rules are written
//! in, the model built from approved knowledge notes, and its compilation to SQL.

pub mod ast;
pub mod compile;
pub mod model;
pub mod parse;

pub use compile::{
    Compiled, MetricCheck, SemDimension, SemFilter, SemanticError, SemanticQuery, check_metric,
    check_rule, compile,
};
pub use model::{BoundTable, Column, Meta, Model, RawDefinition, Standing, ident};
pub use parse::{ParseError, condition, formula};
