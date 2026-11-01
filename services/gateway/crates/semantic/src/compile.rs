//! Checks definitions against the model and compiles semantic queries to SQL.
//!
//! A query has one **grain**: the entity its metrics aggregate (orders, customers…). Every other
//! entity it touches — through a dimension, a filter, a rule — must be reachable from the grain
//! by many-to-one relationships, so joining never multiplies rows ("fan-out"): a sum of order
//! amounts stays a sum of order amounts whatever is grouped by. Metrics over different grains
//! don't mix in one query; the error says to chart them separately.
//!
//! Every identifier comes from the model and is quoted; every value is a bound parameter.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use chrono::NaiveDate;
use lattice_planner::sql::{TypeClass, ident as quote};
use lattice_planner::{
    ColumnRole, CompiledQuery, DEFAULT_ROWS, FilterOp, FilterValue, MAX_ROWS, OutputColumn,
    ParamValue, Scalar, SortDir, TimeBucket, bucket_name, dimension_parts,
};
use uuid::Uuid;

use crate::ast::{AggFunc, ArithOp, Cond, Expr, Literal, Operand, Path, Target};
use crate::model::{BoundTable, Model, RuleDef, Standing, ident};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct SemanticError(pub String);

type Result<T> = std::result::Result<T, SemanticError>;

fn fail<T>(m: impl Into<String>) -> Result<T> {
    Err(SemanticError(m.into()))
}

/// A dimension: an entity's column, optionally bucketed (dates) or binned (numbers).
#[derive(Debug, Clone, PartialEq)]
pub struct SemDimension {
    pub path: Path,
    pub bucket: Option<TimeBucket>,
    pub bin: Option<f64>,
}

/// A filter on `entity.column` (or a bare column of the grain entity).
#[derive(Debug, Clone, PartialEq)]
pub struct SemFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: Option<FilterValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticQuery {
    pub metrics: Vec<Uuid>,
    pub dimensions: Vec<SemDimension>,
    /// Must apply: an unknown column is an error.
    pub filters: Vec<SemFilter>,
    /// Sheet filters and selections: skipped where the column isn't reachable.
    pub context_filters: Vec<SemFilter>,
    pub sort: Vec<(String, SortDir)>,
    pub limit: Option<u32>,
    pub today: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct Compiled {
    pub query: CompiledQuery,
    /// Data versions of every table read, for the result-cache key.
    pub data_versions: Vec<(Uuid, i64)>,
    /// Every definition used, `id@version`, for the result-cache key.
    pub logic: String,
    /// Definitions used that aren't approved (the "Uses unapproved logic" badge).
    pub unapproved: Vec<Uuid>,
    pub grain: String,
}

/// What a metric needs: its grain and the definitions it uses.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricCheck {
    pub grain: String,
    pub uses: Vec<Uuid>,
}

struct Analysis<'m> {
    model: &'m Model,
    /// note id → (version, standing) of every definition used.
    uses: BTreeMap<Uuid, (i32, Standing)>,
    grains: Vec<String>,
    entities: HashSet<String>,
    stack: Vec<Uuid>,
}

impl<'m> Analysis<'m> {
    fn new(model: &'m Model) -> Self {
        Self {
            model,
            uses: BTreeMap::new(),
            grains: Vec::new(),
            entities: HashSet::new(),
            stack: Vec::new(),
        }
    }

    fn entity(&self, name: &str) -> Result<&'m BoundTable> {
        let Some(e) = self.model.entity(name) else {
            return fail(format!(
                "there's no entity called \"{name}\" (entities are named after their notes, e.g. product_category)"
            ));
        };
        e.table.as_ref().ok_or_else(|| {
            SemanticError(format!(
                "\"{}\" isn't connected to a table in this project yet",
                e.meta.title
            ))
        })
    }

    fn column(&mut self, path: &Path) -> Result<TypeClass> {
        let table = self.entity(&path.entity)?;
        let Some(col) = table
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&path.column))
        else {
            return fail(format!("{} has no column \"{}\"", path.entity, path.column));
        };
        self.entities.insert(path.entity.clone());
        Ok(TypeClass::of(&col.data_type))
    }

    fn metric(&mut self, id: Uuid) -> Result<()> {
        let Some(m) = self.model.metric_by_id(id) else {
            return fail("that metric isn't in this project's knowledge");
        };
        if self.stack.contains(&id) {
            return fail(format!(
                "\"{}\" refers to itself through other metrics",
                m.meta.title
            ));
        }
        self.uses.insert(id, (m.meta.version, m.meta.standing));
        let Some(formula) = &m.formula else {
            return fail(format!("\"{}\" has no formula yet", m.meta.title));
        };
        let expr = formula
            .as_ref()
            .map_err(|e| SemanticError(format!("\"{}\": {e}", m.meta.title)))?;
        self.stack.push(id);
        let r = self.expr(expr);
        self.stack.pop();
        r
    }

    fn expr(&mut self, e: &Expr) -> Result<()> {
        match e {
            Expr::Number(_) => Ok(()),
            Expr::Binary { left, right, .. } => {
                self.expr(left)?;
                self.expr(right)
            }
            Expr::Metric(title) => {
                let id = self
                    .model
                    .metric(title)
                    .map(|m| m.meta.note_id)
                    .ok_or_else(|| {
                        SemanticError(format!("there's no metric called \"{title}\""))
                    })?;
                self.metric(id)
            }
            Expr::Agg {
                func,
                target,
                filter,
            } => {
                let grain = target.entity().to_owned();
                match target {
                    Target::Column(p) => {
                        let class = self.column(p)?;
                        if matches!(func, AggFunc::Sum | AggFunc::Avg) && !class.is_numeric() {
                            return fail(format!(
                                "{} needs a number, but {p} isn't one",
                                func.name()
                            ));
                        }
                    }
                    Target::Rows(e) => {
                        self.entity(e)?;
                        self.entities.insert(e.clone());
                    }
                }
                if !self.grains.contains(&grain) {
                    self.grains.push(grain);
                }
                if let Some(c) = filter {
                    self.cond(c)?;
                }
                Ok(())
            }
        }
    }

    fn cond(&mut self, c: &Cond) -> Result<()> {
        match c {
            Cond::And(a, b) | Cond::Or(a, b) => {
                self.cond(a)?;
                self.cond(b)
            }
            Cond::Not(a) => self.cond(a),
            Cond::Cmp { left, right, .. } => {
                let l = self.operand(left)?;
                let r = self.operand(right)?;
                if let (Some(l), Some(r)) = (l, r)
                    && l.is_numeric() != r.is_numeric()
                    && !(l.is_temporal() && r.is_temporal())
                {
                    return fail("compares values of different types");
                }
                Ok(())
            }
            Cond::In { path, .. } | Cond::IsNull { path, .. } => self.column(path).map(|_| ()),
            Cond::Rule(title) => {
                let rule = self
                    .model
                    .rule(title)
                    .ok_or_else(|| SemanticError(format!("there's no rule called \"{title}\"")))?;
                self.rule(rule)
            }
        }
    }

    fn rule(&mut self, rule: &RuleDef) -> Result<()> {
        let id = rule.meta.note_id;
        if self.stack.contains(&id) {
            return fail(format!("\"{}\" refers to itself", rule.meta.title));
        }
        self.uses
            .insert(id, (rule.meta.version, rule.meta.standing));
        let Some(cond) = &rule.condition else {
            return fail(format!("\"{}\" has no condition", rule.meta.title));
        };
        let cond = cond
            .as_ref()
            .map_err(|e| SemanticError(format!("\"{}\": {e}", rule.meta.title)))?;
        self.stack.push(id);
        let r = self.cond(cond);
        self.stack.pop();
        r
    }

    /// The type of a path operand (None for literals and TODAY).
    fn operand(&mut self, o: &Operand) -> Result<Option<TypeClass>> {
        match o {
            Operand::Path(p) => self.column(p).map(Some),
            Operand::Literal(Literal::Number(_)) => Ok(Some(TypeClass::Float)),
            Operand::Literal(Literal::Text(_)) => Ok(None),
            Operand::Literal(Literal::Bool(_)) => Ok(Some(TypeClass::Boolean)),
            Operand::Literal(Literal::Date(_)) | Operand::Today { .. } => Ok(Some(TypeClass::Date)),
        }
    }
}

/// Checks a metric: it parses, everything it names exists, and it aggregates one entity.
pub fn check_metric(model: &Model, id: Uuid) -> Result<MetricCheck> {
    let mut a = Analysis::new(model);
    a.metric(id)?;
    let grain = single_grain(&a.grains)?;
    joins(model, &grain, &a.entities)?;
    Ok(MetricCheck {
        grain,
        uses: a.uses.keys().copied().collect(),
    })
}

/// Checks a rule's condition (it may name any entities joinable from each other).
pub fn check_rule(model: &Model, rule: &RuleDef) -> Result<()> {
    let mut a = Analysis::new(model);
    a.rule(rule)?;
    Ok(())
}

fn single_grain(grains: &[String]) -> Result<String> {
    match grains {
        [] => fail("the formula doesn't aggregate anything"),
        [g] => Ok(g.clone()),
        many => fail(format!(
            "mixes aggregates over {}: a metric counts one kind of thing (chart them separately, or define each as its own metric)",
            many.join(" and ")
        )),
    }
}

/// One step of a join path: `from` (already joined) to `to` via `from_col = to_col`.
#[derive(Debug, Clone)]
struct Join {
    from: String,
    to: String,
    from_col: String,
    to_col: String,
}

/// Joins from the grain to every needed entity, following relationships many-to-one only.
fn joins(model: &Model, grain: &str, needed: &HashSet<String>) -> Result<Vec<Join>> {
    let mut edges: HashMap<&str, Vec<Join>> = HashMap::new();
    for r in &model.relationships {
        if let (Ok(from), Ok(to)) = (&r.from, &r.to) {
            edges.entry(from.entity.as_str()).or_default().push(Join {
                from: from.entity.clone(),
                to: to.entity.clone(),
                from_col: from.column.clone(),
                to_col: to.column.clone(),
            });
        }
    }
    // Breadth-first: the shortest path to each entity.
    let mut via: HashMap<String, Join> = HashMap::new();
    let mut seen = HashSet::from([grain.to_owned()]);
    let mut queue = VecDeque::from([grain.to_owned()]);
    while let Some(e) = queue.pop_front() {
        for j in edges.get(e.as_str()).into_iter().flatten() {
            if seen.insert(j.to.clone()) {
                via.insert(j.to.clone(), j.clone());
                queue.push_back(j.to.clone());
            }
        }
    }
    let mut out: Vec<Join> = Vec::new();
    let mut wanted: Vec<&String> = needed.iter().filter(|e| e.as_str() != grain).collect();
    wanted.sort();
    for target in wanted {
        let mut chain = Vec::new();
        let mut at = target.clone();
        while at != grain {
            let Some(j) = via.get(&at) else {
                return fail(format!(
                    "can't relate {grain} to {target}: no many-to-one relationship leads there (add a relationship note, e.g. {grain}.<column> → {target}.<column>)"
                ));
            };
            chain.push(j.clone());
            at = j.from.clone();
        }
        for j in chain.into_iter().rev() {
            if !out.iter().any(|o| o.to == j.to) {
                out.push(j);
            }
        }
    }
    Ok(out)
}

struct Sql<'m> {
    model: &'m Model,
    aliases: HashMap<String, String>,
    params: Vec<ParamValue>,
    today: NaiveDate,
    stack: Vec<Uuid>,
}

impl Sql<'_> {
    fn table(&self, entity: &str) -> Result<&BoundTable> {
        self.model
            .entity(entity)
            .and_then(|e| e.table.as_ref())
            .ok_or_else(|| SemanticError(format!("{entity} isn't bound")))
    }

    fn column(&self, p: &Path) -> Result<(String, TypeClass)> {
        let alias = self
            .aliases
            .get(&p.entity)
            .ok_or_else(|| SemanticError(format!("{} isn't part of this query", p.entity)))?;
        let col = self
            .table(&p.entity)?
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&p.column))
            .ok_or_else(|| SemanticError(format!("{p} doesn't exist")))?;
        Ok((
            format!("{}.{}", quote(alias), quote(&col.name)),
            TypeClass::of(&col.data_type),
        ))
    }

    fn bind(&mut self, lit: &Literal, class: Option<TypeClass>) -> Result<String> {
        let target = class.and_then(TypeClass::cast_target);
        let p = match (lit, class) {
            (Literal::Number(n), Some(TypeClass::Integer)) if n.fract() == 0.0 => {
                ParamValue::BigInt(*n as i64)
            }
            (Literal::Number(n), _) => ParamValue::Double(*n),
            (Literal::Bool(b), _) => ParamValue::Bool(*b),
            (Literal::Date(d), _) => {
                self.params.push(ParamValue::Text(d.clone()));
                return Ok(format!("CAST(? AS {})", target.unwrap_or("DATE")));
            }
            (Literal::Text(t), Some(c)) if c.is_temporal() => {
                self.params.push(ParamValue::Text(t.clone()));
                return Ok(format!("CAST(? AS {})", target.unwrap_or("DATE")));
            }
            (Literal::Text(t), _) => ParamValue::Text(t.clone()),
        };
        self.params.push(p);
        Ok("?".into())
    }

    fn operand(
        &mut self,
        o: &Operand,
        other: Option<TypeClass>,
    ) -> Result<(String, Option<TypeClass>)> {
        match o {
            Operand::Path(p) => self.column(p).map(|(s, c)| (s, Some(c))),
            Operand::Literal(l) => Ok((self.bind(l, other)?, None)),
            Operand::Today { offset_days } => {
                self.params.push(ParamValue::Text(self.today.to_string()));
                let sql = match offset_days {
                    0 => "CAST(? AS DATE)".to_owned(),
                    n if *n < 0 => format!("(CAST(? AS DATE) - INTERVAL {} DAY)", -n),
                    n => format!("(CAST(? AS DATE) + INTERVAL {n} DAY)"),
                };
                Ok((sql, Some(TypeClass::Date)))
            }
        }
    }

    fn cond(&mut self, c: &Cond) -> Result<String> {
        Ok(match c {
            Cond::And(a, b) => format!("({} AND {})", self.cond(a)?, self.cond(b)?),
            Cond::Or(a, b) => format!("({} OR {})", self.cond(a)?, self.cond(b)?),
            Cond::Not(a) => format!("(NOT {})", self.cond(a)?),
            Cond::Cmp { left, op, right } => {
                // Bind each literal against the other side's type, in text order.
                let left_class = match left {
                    Operand::Path(p) => Some(self.column(p)?.1),
                    _ => None,
                };
                let right_class = match right {
                    Operand::Path(p) => Some(self.column(p)?.1),
                    _ => None,
                };
                let (l, _) = self.operand(left, right_class)?;
                let (r, _) = self.operand(right, left_class)?;
                format!("{l} {} {r}", op.sql())
            }
            Cond::In {
                path,
                values,
                negated,
            } => {
                let (col, class) = self.column(path)?;
                let list = values
                    .iter()
                    .map(|v| self.bind(v, Some(class)))
                    .collect::<Result<Vec<_>>>()?
                    .join(", ");
                format!("{col} {}IN ({list})", if *negated { "NOT " } else { "" })
            }
            Cond::IsNull { path, negated } => {
                format!(
                    "{} IS {}NULL",
                    self.column(path)?.0,
                    if *negated { "NOT " } else { "" }
                )
            }
            Cond::Rule(title) => {
                let rule = self
                    .model
                    .rule(title)
                    .ok_or_else(|| SemanticError(format!("no rule \"{title}\"")))?;
                let Some(Ok(cond)) = &rule.condition else {
                    return fail(format!("\"{title}\" has no valid condition"));
                };
                if self.stack.contains(&rule.meta.note_id) {
                    return fail(format!("\"{title}\" refers to itself"));
                }
                self.stack.push(rule.meta.note_id);
                let sql = self.cond(cond)?;
                self.stack.pop();
                format!("({sql})")
            }
        })
    }

    fn expr(&mut self, e: &Expr) -> Result<String> {
        Ok(match e {
            Expr::Number(n) => format!("{n:?}"),
            Expr::Binary { op, left, right } => {
                let (l, r) = (self.expr(left)?, self.expr(right)?);
                match op {
                    ArithOp::Add => format!("({l} + {r})"),
                    ArithOp::Sub => format!("({l} - {r})"),
                    ArithOp::Mul => format!("({l} * {r})"),
                    // Never integer division; dividing by zero gives no value, not an error.
                    ArithOp::Div => format!("(CAST({l} AS DOUBLE) / NULLIF({r}, 0))"),
                }
            }
            Expr::Metric(title) => {
                let m = self
                    .model
                    .metric(title)
                    .ok_or_else(|| SemanticError(format!("no metric \"{title}\"")))?;
                let Some(Ok(expr)) = &m.formula else {
                    return fail(format!("\"{title}\" has no valid formula"));
                };
                if self.stack.contains(&m.meta.note_id) {
                    return fail(format!("\"{title}\" refers to itself"));
                }
                self.stack.push(m.meta.note_id);
                let sql = self.expr(expr)?;
                self.stack.pop();
                format!("({sql})")
            }
            Expr::Agg {
                func,
                target,
                filter,
            } => {
                let arg = match target {
                    Target::Rows(_) => "*".to_owned(),
                    Target::Column(p) => self.column(p)?.0,
                };
                let agg = match func {
                    AggFunc::Sum => format!("sum({arg})"),
                    AggFunc::Avg => format!("avg({arg})"),
                    AggFunc::Min => format!("min({arg})"),
                    AggFunc::Max => format!("max({arg})"),
                    AggFunc::Count => format!("count({arg})"),
                    AggFunc::CountDistinct => format!("count(DISTINCT {arg})"),
                };
                match filter {
                    Some(c) => format!("{agg} FILTER (WHERE {})", self.cond(c)?),
                    None => agg,
                }
            }
        })
    }

    fn filter(&mut self, col: &str, class: TypeClass, f: &SemFilter) -> Result<String> {
        let scalar = |s: &Scalar| match s {
            Scalar::Bool(b) => Literal::Bool(*b),
            Scalar::Number(n) => Literal::Number(*n),
            Scalar::Text(t) => Literal::Text(t.clone()),
        };
        let one = |f: &SemFilter| match &f.value {
            Some(FilterValue::One(s)) => Ok(scalar(s)),
            _ => fail(format!("the filter on {} needs one value", f.column)),
        };
        let many = |f: &SemFilter| match &f.value {
            Some(FilterValue::Many(v)) if !v.is_empty() && v.len() <= 1000 => {
                Ok(v.iter().map(scalar).collect::<Vec<_>>())
            }
            _ => fail(format!("the filter on {} needs a list of values", f.column)),
        };
        let cmp = |op: &str, this: &mut Self, f: &SemFilter| -> Result<String> {
            let p = this.bind(&one(f)?, Some(class))?;
            Ok(format!("{col} {op} {p}"))
        };
        match f.op {
            FilterOp::Eq => cmp("=", self, f),
            FilterOp::Neq => cmp("<>", self, f),
            FilterOp::Gt => cmp(">", self, f),
            FilterOp::Gte => cmp(">=", self, f),
            FilterOp::Lt => cmp("<", self, f),
            FilterOp::Lte => cmp("<=", self, f),
            FilterOp::In | FilterOp::NotIn => {
                let list = many(f)?
                    .iter()
                    .map(|v| self.bind(v, Some(class)))
                    .collect::<Result<Vec<_>>>()?
                    .join(", ");
                Ok(format!(
                    "{col} {}IN ({list})",
                    if f.op == FilterOp::NotIn { "NOT " } else { "" }
                ))
            }
            FilterOp::Between => match many(f)?.as_slice() {
                [lo, hi] => {
                    let (lo, hi) = (self.bind(lo, Some(class))?, self.bind(hi, Some(class))?);
                    Ok(format!("{col} BETWEEN {lo} AND {hi}"))
                }
                _ => fail(format!("the filter on {} needs two values", f.column)),
            },
            FilterOp::IsNull => Ok(format!("{col} IS NULL")),
            FilterOp::IsNotNull => Ok(format!("{col} IS NOT NULL")),
            FilterOp::Contains => match one(f)? {
                Literal::Text(t) => {
                    self.params.push(ParamValue::Text(t));
                    Ok(format!("contains(lower(CAST({col} AS VARCHAR)), lower(?))"))
                }
                _ => fail("contains needs text"),
            },
        }
    }
}

/// `entity.column`, or a bare column meaning the grain entity's.
fn filter_path(column: &str, grain: &str) -> Path {
    match column.split_once('.') {
        Some((e, c)) => Path {
            entity: e.trim().to_lowercase(),
            column: c.trim().to_lowercase(),
        },
        None => Path {
            entity: grain.to_owned(),
            column: column.trim().to_lowercase(),
        },
    }
}

fn unique(names: &mut Vec<String>, base: String) -> String {
    let mut name = base.clone();
    let mut n = 2;
    while names.contains(&name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    names.push(name.clone());
    name
}

/// Compiles a semantic query: metrics grouped by dimensions, with filters and every applicable
/// standing rule, joined from the grain entity.
pub fn compile(model: &Model, q: &SemanticQuery) -> Result<Compiled> {
    if q.metrics.is_empty() {
        return fail("choose at least one metric");
    }
    let mut a = Analysis::new(model);
    for id in &q.metrics {
        a.metric(*id)?;
    }
    let grain = single_grain(&a.grains)?;
    for d in &q.dimensions {
        let class = a.column(&d.path)?;
        if d.bucket.is_some() && !class.is_temporal() {
            return fail(format!(
                "{} isn't a date, so it can't be grouped by {}",
                d.path,
                d.bucket.map(bucket_name).unwrap_or("")
            ));
        }
        if d.bin.is_some() && !class.is_numeric() {
            return fail(format!("{} isn't a number, so it can't be binned", d.path));
        }
    }
    for f in &q.filters {
        a.column(&filter_path(&f.column, &grain))?;
    }
    // Reachable entities decide which context filters and standing rules apply.
    let mut reachable = HashSet::from([grain.clone()]);
    for e in &model.entities {
        if e.table.is_some() && joins(model, &grain, &HashSet::from([e.ident.clone()])).is_ok() {
            reachable.insert(e.ident.clone());
        }
    }
    let context: Vec<(&SemFilter, Path)> = q
        .context_filters
        .iter()
        .map(|f| (f, filter_path(&f.column, &grain)))
        .filter(|(_, p)| {
            reachable.contains(&p.entity)
                && model
                    .entity(&p.entity)
                    .and_then(|e| e.table.as_ref())
                    .is_some_and(|t| {
                        t.columns
                            .iter()
                            .any(|c| c.name.eq_ignore_ascii_case(&p.column))
                    })
        })
        .collect();
    for (_, p) in &context {
        a.column(p)?;
    }
    let standing: Vec<&RuleDef> = model
        .rules
        .iter()
        .filter(|r| r.always && r.meta.standing == Standing::Approved)
        .filter(|r| {
            let mut probe = Analysis::new(model);
            probe.rule(r).is_ok() && probe.entities.iter().all(|e| reachable.contains(e))
        })
        .collect();
    for r in &standing {
        a.rule(r)?;
    }

    let plan = joins(model, &grain, &a.entities)?;
    let mut aliases = HashMap::from([(grain.clone(), "e0".to_owned())]);
    for (i, j) in plan.iter().enumerate() {
        aliases.insert(j.to.clone(), format!("e{}", i + 1));
    }
    let mut sql = Sql {
        model,
        aliases,
        params: Vec::new(),
        today: q.today,
        stack: Vec::new(),
    };

    // Text order matters for `?` parameters: dimensions, measures, then WHERE.
    let mut names = Vec::new();
    let mut columns = Vec::new();
    let mut dims = Vec::new();
    for (i, d) in q.dimensions.iter().enumerate() {
        let (col, class) = sql.column(&d.path)?;
        let alias = unique(
            &mut names,
            match (d.bucket, d.bin) {
                (Some(b), _) => format!("{}_{}", d.path.column, bucket_name(b)),
                (None, Some(_)) => format!("{}_bin", d.path.column),
                (None, None) => d.path.column.clone(),
            },
        );
        let expr = match d.bin {
            Some(w) => format!("floor({col} / {w:e}) * {w:e}"),
            None => col,
        };
        dims.push(dimension_parts(
            i,
            &expr,
            class,
            if d.bin.is_some() { None } else { d.bucket },
            &alias,
        ));
        columns.push(OutputColumn {
            name: alias,
            role: ColumnRole::Dimension,
        });
    }
    let mut measures = Vec::new();
    for id in &q.metrics {
        let m = model
            .metric_by_id(*id)
            .ok_or_else(|| SemanticError("unknown metric".into()))?;
        let Some(Ok(expr)) = &m.formula else {
            return fail(format!("\"{}\" has no valid formula", m.meta.title));
        };
        sql.stack.push(m.meta.note_id);
        let e = sql.expr(expr)?;
        sql.stack.pop();
        let name = unique(&mut names, ident(&m.meta.title));
        measures.push((e, name.clone()));
        columns.push(OutputColumn {
            name,
            role: ColumnRole::Measure,
        });
    }

    let grain_table = sql.table(&grain)?;
    let mut from = format!("{} AS {}", grain_table.reference, quote("e0"));
    for j in &plan {
        let to = sql.table(&j.to)?;
        from.push_str(&format!(
            " LEFT JOIN {} AS {} ON {}.{} = {}.{}",
            to.reference,
            quote(&sql.aliases[&j.to]),
            quote(&sql.aliases[&j.from]),
            quote(&j.from_col),
            quote(&sql.aliases[&j.to]),
            quote(&j.to_col),
        ));
    }

    let mut wheres = Vec::new();
    for f in &q.filters {
        let p = filter_path(&f.column, &grain);
        let (col, class) = sql.column(&p)?;
        wheres.push(sql.filter(&col, class, f)?);
    }
    for (f, p) in &context {
        let (col, class) = sql.column(p)?;
        wheres.push(sql.filter(&col, class, f)?);
    }
    for r in &standing {
        if let Some(Ok(c)) = &r.condition {
            let cond = sql.cond(c)?;
            let mut probe = Analysis::new(model);
            probe.rule(r)?;
            // A standing rule about a joined entity only removes rows it positively matches:
            // an order without a customer row isn't an internal customer's order.
            if probe.entities.iter().all(|e| *e == grain) {
                wheres.push(cond);
            } else {
                wheres.push(format!("COALESCE({cond}, TRUE)"));
            }
        }
    }
    let where_sql = if wheres.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", wheres.join(" AND "))
    };
    let group_by = if dims.is_empty() { "" } else { " GROUP BY ALL" };

    let two_level = dims.iter().any(|d| d.outer.is_some());
    let select = if two_level {
        let inner = dims
            .iter()
            .flat_map(|d| d.inner.iter().cloned())
            .chain(
                measures
                    .iter()
                    .enumerate()
                    .map(|(j, (e, _))| format!("{e} AS \"m{j}\"")),
            )
            .collect::<Vec<_>>()
            .join(", ");
        let outer = dims
            .iter()
            .map(|d| d.outer.clone().unwrap_or_else(|| d.simple_outer()))
            .chain(
                measures
                    .iter()
                    .enumerate()
                    .map(|(j, (_, n))| format!("\"m{j}\" AS {}", quote(n))),
            )
            .collect::<Vec<_>>()
            .join(", ");
        format!("SELECT {outer} FROM (SELECT {inner} FROM {from}{where_sql}{group_by})")
    } else {
        let items = dims
            .iter()
            .map(|d| format!("{} AS {}", d.expr, quote(&d.alias)))
            .chain(measures.iter().map(|(e, n)| format!("{e} AS {}", quote(n))))
            .collect::<Vec<_>>()
            .join(", ");
        format!("SELECT {items} FROM {from}{where_sql}{group_by}")
    };

    let order: Vec<String> = if q.sort.is_empty() {
        columns
            .iter()
            .filter(|c| c.role == ColumnRole::Dimension)
            .map(|c| format!("{} ASC NULLS LAST", quote(&c.name)))
            .collect()
    } else {
        q.sort
            .iter()
            .map(|(by, dir)| {
                if !columns.iter().any(|c| &c.name == by) {
                    return fail(format!("can't sort by \"{by}\""));
                }
                Ok(format!(
                    "{} {} NULLS LAST",
                    quote(by),
                    if *dir == SortDir::Desc { "DESC" } else { "ASC" }
                ))
            })
            .collect::<Result<_>>()?
    };
    let order = if order.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", order.join(", "))
    };
    let row_limit = q.limit.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);

    let mut data_versions: Vec<(Uuid, i64)> = a
        .entities
        .iter()
        .chain(std::iter::once(&grain))
        .filter_map(|e| model.entity(e).and_then(|e| e.table.as_ref()))
        .map(|t| (t.table_id, t.data_version))
        .collect();
    data_versions.sort();
    data_versions.dedup();
    Ok(Compiled {
        query: CompiledQuery {
            sql: format!("{select}{order} LIMIT {}", u64::from(row_limit) + 1),
            params: sql.params,
            columns,
            row_limit,
        },
        data_versions,
        logic: a
            .uses
            .iter()
            .map(|(id, (v, _))| format!("{id}@{v}"))
            .collect::<Vec<_>>()
            .join(","),
        unapproved: a
            .uses
            .iter()
            .filter(|(_, (_, s))| *s == Standing::Unapproved)
            .map(|(id, _)| *id)
            .collect(),
        grain,
    })
}
