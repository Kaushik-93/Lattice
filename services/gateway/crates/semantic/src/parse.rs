//! Parser for formulas and conditions. Grammar (keywords are case-insensitive):
//!
//! ```text
//! formula   := term (("+" | "-") term)*
//! term      := factor (("*" | "/") factor)*
//! factor    := NUMBER | "(" formula ")" | aggregate | "[[" title "]]"
//! aggregate := AGG "(" (path | entity) ")" (WHERE condition)?
//! AGG       := SUM | AVG | MIN | MAX | COUNT | COUNT DISTINCT
//! condition := and (OR and)*
//! and       := not (AND not)*
//! not       := NOT not | "(" condition ")" | "[[" title "]]" | predicate
//! predicate := path [NOT] IN "(" literal ("," literal)* ")"
//!            | path IS [NOT] NULL
//!            | operand ("=" | "!=" | "<>" | "<" | "<=" | ">" | ">=") operand
//! operand   := path | literal | TODAY (("+" | "-") NUMBER DAYS)?
//! literal   := 'text' | NUMBER | TRUE | FALSE | DATE 'YYYY-MM-DD'
//! path      := identifier "." identifier
//! ```

use crate::ast::{AggFunc, ArithOp, CmpOp, Cond, Expr, Literal, Operand, Path, Target};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (at character {at})")]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the formula.
    pub at: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Number(f64),
    Text(String),
    Link(String),
    Sym(&'static str),
    End,
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

// Longest first, so "<=" isn't read as "<" then "=".
const SYMBOLS: [&str; 15] = [
    "<=", ">=", "!=", "<>", "<", ">", "(", ")", ",", ".", "+", "-", "*", "/", "=",
];

impl<'a> Lexer<'a> {
    fn tokens(src: &'a str) -> Result<Vec<(Tok, usize)>, ParseError> {
        let mut lexer = Lexer { src, pos: 0 };
        let mut out = Vec::new();
        loop {
            let (tok, at) = lexer.next()?;
            let end = tok == Tok::End;
            out.push((tok, at));
            if end {
                return Ok(out);
            }
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn next(&mut self) -> Result<(Tok, usize), ParseError> {
        let trimmed = self.rest().trim_start();
        self.pos = self.src.len() - trimmed.len();
        let at = self.pos;
        let Some(c) = trimmed.chars().next() else {
            return Ok((Tok::End, at));
        };
        if trimmed.starts_with("[[") {
            let Some(end) = trimmed.find("]]") else {
                return Err(ParseError {
                    message: "a [[link]] isn't closed".into(),
                    at,
                });
            };
            let title = trimmed[2..end]
                .split('|')
                .next()
                .unwrap_or("")
                .trim()
                .to_owned();
            if title.is_empty() {
                return Err(ParseError {
                    message: "an empty [[link]]".into(),
                    at,
                });
            }
            self.pos += end + 2;
            return Ok((Tok::Link(title), at));
        }
        if c == '\'' {
            let mut text = String::new();
            let mut chars = trimmed[1..].char_indices();
            while let Some((i, ch)) = chars.next() {
                if ch == '\'' {
                    // '' is an escaped quote.
                    if trimmed[1 + i + 1..].starts_with('\'') {
                        text.push('\'');
                        chars.next();
                        continue;
                    }
                    self.pos += 1 + i + 1;
                    return Ok((Tok::Text(text), at));
                }
                text.push(ch);
            }
            return Err(ParseError {
                message: "a 'text' value isn't closed".into(),
                at,
            });
        }
        if c.is_ascii_digit() {
            let len = trimmed
                .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
                .unwrap_or(trimmed.len());
            let n: f64 = trimmed[..len].parse().map_err(|_| ParseError {
                message: format!("{:?} isn't a number", &trimmed[..len]),
                at,
            })?;
            self.pos += len;
            return Ok((Tok::Number(n), at));
        }
        if c.is_alphabetic() || c == '_' {
            let len = trimmed
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .unwrap_or(trimmed.len());
            self.pos += len;
            return Ok((Tok::Ident(trimmed[..len].to_owned()), at));
        }
        for sym in SYMBOLS {
            if trimmed.starts_with(sym) {
                self.pos += sym.len();
                return Ok((Tok::Sym(sym), at));
            }
        }
        Err(ParseError {
            message: format!("unexpected {c:?}"),
            at,
        })
    }
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)].0
    }

    fn at(&self) -> usize {
        self.toks[self.i.min(self.toks.len() - 1)].1
    }

    /// Always advances (past the end too: `peek` clamps), so stepping back one is always right.
    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        self.i += 1;
        t
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            at: self.at(),
        })
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    fn kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Tok::Sym(x) if *x == s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, s: &str) -> Result<(), ParseError> {
        if self.sym(s) {
            Ok(())
        } else {
            self.err(format!("expected \"{s}\""))
        }
    }

    fn end(&mut self) -> Result<(), ParseError> {
        match self.peek() {
            Tok::End => Ok(()),
            _ => self.err("unexpected text after the end of the expression"),
        }
    }

    // ---- formulas ----

    fn formula(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.term()?;
        loop {
            let op = if self.sym("+") {
                ArithOp::Add
            } else if self.sym("-") {
                ArithOp::Sub
            } else {
                return Ok(left);
            };
            let right = self.term()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.factor()?;
        loop {
            let op = if self.sym("*") {
                ArithOp::Mul
            } else if self.sym("/") {
                ArithOp::Div
            } else {
                return Ok(left);
            };
            let right = self.factor()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn factor(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Tok::Number(n) => {
                self.bump();
                Ok(Expr::Number(n))
            }
            Tok::Sym("(") => {
                self.bump();
                let e = self.formula()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            Tok::Link(title) => {
                self.bump();
                Ok(Expr::Metric(title))
            }
            Tok::Ident(_) => self.aggregate(),
            _ => self
                .err("expected a number, an aggregate such as SUM(order.amount), or a [[metric]]"),
        }
    }

    fn aggregate(&mut self) -> Result<Expr, ParseError> {
        let func = if self.kw("sum") {
            AggFunc::Sum
        } else if self.kw("avg") {
            AggFunc::Avg
        } else if self.kw("min") {
            AggFunc::Min
        } else if self.kw("max") {
            AggFunc::Max
        } else if self.kw("count") {
            if self.kw("distinct") {
                AggFunc::CountDistinct
            } else {
                AggFunc::Count
            }
        } else {
            return self.err("expected SUM, AVG, MIN, MAX, COUNT or COUNT DISTINCT");
        };
        self.expect_sym("(")?;
        let target = match self.bump() {
            Tok::Ident(entity) => {
                if self.sym(".") {
                    Target::Column(Path {
                        entity: entity.to_lowercase(),
                        column: self.ident("a column")?,
                    })
                } else {
                    Target::Rows(entity.to_lowercase())
                }
            }
            _ => {
                self.i -= 1;
                return self.err("expected entity.column, or an entity to count its rows");
            }
        };
        if matches!(target, Target::Rows(_)) && func != AggFunc::Count {
            return self.err(format!(
                "{} needs a column, e.g. {}(entity.column)",
                func.name(),
                func.name()
            ));
        }
        self.expect_sym(")")?;
        let filter = if self.kw("where") {
            Some(self.condition()?)
        } else {
            None
        };
        Ok(Expr::Agg {
            func,
            target,
            filter,
        })
    }

    fn ident(&mut self, what: &str) -> Result<String, ParseError> {
        match self.bump() {
            Tok::Ident(s) => Ok(s.to_lowercase()),
            _ => {
                self.i -= 1;
                self.err(format!("expected {what}"))
            }
        }
    }

    // ---- conditions ----

    fn condition(&mut self) -> Result<Cond, ParseError> {
        let mut left = self.and()?;
        while self.kw("or") {
            let right = self.and()?;
            left = Cond::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Cond, ParseError> {
        let mut left = self.not()?;
        while self.kw("and") {
            let right = self.not()?;
            left = Cond::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Cond, ParseError> {
        if self.kw("not") {
            return Ok(Cond::Not(Box::new(self.not()?)));
        }
        if self.sym("(") {
            let c = self.condition()?;
            self.expect_sym(")")?;
            return Ok(c);
        }
        if let Tok::Link(title) = self.peek().clone() {
            self.bump();
            return Ok(Cond::Rule(title));
        }
        self.predicate()
    }

    fn predicate(&mut self) -> Result<Cond, ParseError> {
        let left = self.operand()?;
        if let Operand::Path(path) = &left {
            let negated_in = self.is_kw("not") && {
                self.bump();
                true
            };
            if self.kw("in") {
                self.expect_sym("(")?;
                let mut values = vec![self.literal()?];
                while self.sym(",") {
                    values.push(self.literal()?);
                }
                self.expect_sym(")")?;
                return Ok(Cond::In {
                    path: path.clone(),
                    values,
                    negated: negated_in,
                });
            }
            if negated_in {
                return self.err("expected IN after NOT");
            }
            if self.kw("is") {
                let negated = self.kw("not");
                if !self.kw("null") {
                    return self.err("expected NULL");
                }
                return Ok(Cond::IsNull {
                    path: path.clone(),
                    negated,
                });
            }
        }
        let op = match self.peek() {
            Tok::Sym("=") => CmpOp::Eq,
            Tok::Sym("!=") | Tok::Sym("<>") => CmpOp::Ne,
            Tok::Sym("<") => CmpOp::Lt,
            Tok::Sym("<=") => CmpOp::Le,
            Tok::Sym(">") => CmpOp::Gt,
            Tok::Sym(">=") => CmpOp::Ge,
            _ => return self.err("expected a comparison (=, !=, <, <=, >, >=), IN or IS NULL"),
        };
        self.bump();
        let right = self.operand()?;
        Ok(Cond::Cmp { left, op, right })
    }

    fn operand(&mut self) -> Result<Operand, ParseError> {
        if self.kw("today") {
            let sign = if self.sym("+") {
                1
            } else if self.sym("-") {
                -1
            } else {
                return Ok(Operand::Today { offset_days: 0 });
            };
            let Tok::Number(n) = self.bump() else {
                self.i -= 1;
                return self.err("expected a number of days");
            };
            if n.fract() != 0.0 || n > 36_500.0 {
                return self.err("a whole number of days, up to 100 years");
            }
            if !(self.kw("days") || self.kw("day")) {
                return self.err("expected DAYS");
            }
            return Ok(Operand::Today {
                offset_days: sign * n as i64,
            });
        }
        if let Tok::Ident(name) = self.peek().clone()
            && !["true", "false", "date"].contains(&name.to_lowercase().as_str())
        {
            self.bump();
            if !self.sym(".") {
                return self.err(format!(
                    "expected a column: {}.<column>",
                    name.to_lowercase()
                ));
            }
            return Ok(Operand::Path(Path {
                entity: name.to_lowercase(),
                column: self.ident("a column")?,
            }));
        }
        Ok(Operand::Literal(self.literal()?))
    }

    fn literal(&mut self) -> Result<Literal, ParseError> {
        let negative = self.sym("-");
        match self.bump() {
            Tok::Number(n) => Ok(Literal::Number(if negative { -n } else { n })),
            _ if negative => {
                self.i -= 1;
                self.err("expected a number")
            }
            Tok::Text(s) => Ok(Literal::Text(s)),
            Tok::Ident(k) if k.eq_ignore_ascii_case("true") => Ok(Literal::Bool(true)),
            Tok::Ident(k) if k.eq_ignore_ascii_case("false") => Ok(Literal::Bool(false)),
            Tok::Ident(k) if k.eq_ignore_ascii_case("date") => match self.bump() {
                Tok::Text(d) if chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").is_ok() => {
                    Ok(Literal::Date(d))
                }
                _ => {
                    self.i -= 1;
                    self.err("expected a date like DATE '2026-08-01'")
                }
            },
            _ => {
                self.i -= 1;
                self.err("expected a value: 'text', a number, TRUE, FALSE or DATE 'YYYY-MM-DD'")
            }
        }
    }
}

const MAX_FORMULA: usize = 2_000;

fn parser(src: &str) -> Result<Parser, ParseError> {
    if src.len() > MAX_FORMULA {
        return Err(ParseError {
            message: format!("formulas are at most {MAX_FORMULA} characters"),
            at: MAX_FORMULA,
        });
    }
    Ok(Parser {
        toks: Lexer::tokens(src)?,
        i: 0,
    })
}

/// Parses a metric formula.
pub fn formula(src: &str) -> Result<Expr, ParseError> {
    let mut p = parser(src)?;
    if matches!(p.peek(), Tok::End) {
        return p.err("the formula is empty");
    }
    let e = p.formula()?;
    p.end()?;
    Ok(e)
}

/// Parses a rule's condition.
pub fn condition(src: &str) -> Result<Cond, ParseError> {
    let mut p = parser(src)?;
    if matches!(p.peek(), Tok::End) {
        return p.err("the condition is empty");
    }
    let c = p.condition()?;
    p.end()?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(e: &str, c: &str) -> Path {
        Path {
            entity: e.into(),
            column: c.into(),
        }
    }

    #[test]
    fn aggregates_with_filters() {
        assert_eq!(
            formula("SUM(order.amount) WHERE order.status = 'settled'").unwrap(),
            Expr::Agg {
                func: AggFunc::Sum,
                target: Target::Column(path("order", "amount")),
                filter: Some(Cond::Cmp {
                    left: Operand::Path(path("order", "status")),
                    op: CmpOp::Eq,
                    right: Operand::Literal(Literal::Text("settled".into())),
                }),
            }
        );
        assert!(
            matches!(formula("count(Order)").unwrap(), Expr::Agg { func: AggFunc::Count, target: Target::Rows(e), .. } if e == "order")
        );
        assert!(matches!(
            formula("COUNT DISTINCT(order.customer_id)").unwrap(),
            Expr::Agg {
                func: AggFunc::CountDistinct,
                ..
            }
        ));
    }

    #[test]
    fn arithmetic_precedence_and_metric_references() {
        // [[Net revenue]] / COUNT(order) WHERE ... : the WHERE belongs to COUNT.
        let e =
            formula("[[Net revenue]] / COUNT(order) WHERE [[Settled orders only]] + 1").unwrap();
        let Expr::Binary {
            op: ArithOp::Add,
            left,
            right,
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(*right, Expr::Number(1.0));
        let Expr::Binary {
            op: ArithOp::Div,
            left: num,
            right: den,
        } = *left
        else {
            panic!()
        };
        assert_eq!(*num, Expr::Metric("Net revenue".into()));
        assert!(
            matches!(*den, Expr::Agg { filter: Some(Cond::Rule(ref r)), .. } if r == "Settled orders only")
        );
    }

    #[test]
    fn conditions() {
        let c = condition("NOT (customer.is_internal = TRUE) AND order.region IN ('EMEA', 'APAC') OR order.note IS NOT NULL").unwrap();
        assert!(matches!(c, Cond::Or(_, _)));
        let c = condition("order.order_date >= TODAY - 90 DAYS").unwrap();
        assert!(matches!(
            c,
            Cond::Cmp {
                right: Operand::Today { offset_days: -90 },
                ..
            }
        ));
        let c = condition("order.order_date >= DATE '2026-02-01' and order.amount > -5").unwrap();
        assert!(matches!(c, Cond::And(_, _)));
        let c = condition("order.region NOT IN ('AMER')").unwrap();
        assert!(matches!(c, Cond::In { negated: true, .. }));
        let c = condition("order.note = 'it''s'").unwrap();
        assert!(
            matches!(c, Cond::Cmp { right: Operand::Literal(Literal::Text(ref t)), .. } if t == "it's")
        );
    }

    #[test]
    fn errors_say_what_and_where() {
        let e = formula("SUM(order.amount").unwrap_err();
        assert_eq!((e.message.as_str(), e.at), ("expected \")\"", 16));
        let e = formula("SUM(order)").unwrap_err();
        assert!(e.message.contains("needs a column"), "{e}");
        let e = formula("SUM(order.amount) WHERE order.status").unwrap_err();
        assert!(e.message.contains("comparison"), "{e}");
        let e = formula("SUM(order.amount) extra").unwrap_err();
        assert!(e.message.contains("after the end"));
        assert!(
            formula("SUM(order.amount) WHERE order.status = 'x")
                .unwrap_err()
                .message
                .contains("isn't closed")
        );
        assert!(formula("").is_err());
        assert!(condition("order.d >= DATE '2026-13-01'").is_err());
        assert!(condition("order.d >= TODAY - 3 weeks").is_err());
        // SQL never gets through as text.
        assert!(formula("SUM(order.amount); DROP TABLE x").is_err());
    }
}
