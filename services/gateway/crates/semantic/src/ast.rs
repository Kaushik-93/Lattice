//! The expression language's syntax tree (docs/adr/0006). Written by people and the AI alike;
//! parsed and checked by the gateway, never passed to a database as text.

use serde::Serialize;

/// `entity.column`, e.g. `order.amount`. Entities are named after their note's title
/// (lowercased, spaces as underscores: "Product category" → `product_category`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Path {
    pub entity: String,
    pub column: String,
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.entity, self.column)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AggFunc {
    Sum,
    Avg,
    Min,
    Max,
    Count,
    CountDistinct,
}

impl AggFunc {
    pub const fn name(self) -> &'static str {
        match self {
            AggFunc::Sum => "SUM",
            AggFunc::Avg => "AVG",
            AggFunc::Min => "MIN",
            AggFunc::Max => "MAX",
            AggFunc::Count => "COUNT",
            AggFunc::CountDistinct => "COUNT DISTINCT",
        }
    }
}

/// What an aggregate reads: a column, or (for `COUNT(order)`) the entity's rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Target {
    Column(Path),
    Rows(String),
}

impl Target {
    pub fn entity(&self) -> &str {
        match self {
            Target::Column(p) => &p.entity,
            Target::Rows(e) => e,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// A metric formula.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Expr {
    Number(f64),
    Agg {
        func: AggFunc,
        target: Target,
        filter: Option<Cond>,
    },
    /// `[[Net revenue]]`: another metric.
    Metric(String),
    Binary {
        op: ArithOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    pub const fn sql(self) -> &'static str {
        match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "<>",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Literal {
    Text(String),
    Number(f64),
    Bool(bool),
    /// `DATE '2026-08-01'`.
    Date(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Operand {
    Path(Path),
    Literal(Literal),
    /// `TODAY`, `TODAY - 90 DAYS`: relative to the day the query runs (UTC).
    Today {
        offset_days: i64,
    },
}

/// A condition: a metric's `WHERE`, or a rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Cond {
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
    Cmp {
        left: Operand,
        op: CmpOp,
        right: Operand,
    },
    In {
        path: Path,
        values: Vec<Literal>,
        negated: bool,
    },
    IsNull {
        path: Path,
        negated: bool,
    },
    /// `[[Settled orders only]]`: a rule's condition.
    Rule(String),
}
