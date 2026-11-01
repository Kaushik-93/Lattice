use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::sql::{TypeClass, ident};
use crate::{
    Aggregation, ColumnInfo, DEFAULT_ROWS, Dimension, Filter, FilterOp, FilterValue, MAX_ROWS,
    Measure, PlanError, Scalar, SortDir, TableQuery, TableSchema, TimeBucket,
};

/// A value bound to a `?` placeholder, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Bool(bool),
    BigInt(i64),
    Double(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputColumn {
    pub name: String,
    pub role: ColumnRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ColumnRole {
    Dimension,
    Measure,
    Column,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledQuery {
    pub sql: String,
    pub params: Vec<ParamValue>,
    pub columns: Vec<OutputColumn>,
    /// Rows to return. The SQL fetches one more so truncation can be detected.
    pub row_limit: u32,
}

pub fn compile(query: &TableQuery, table: &TableSchema) -> Result<CompiledQuery, PlanError> {
    let mut ctx = Ctx {
        table,
        params: Vec::new(),
    };
    let row_limit = query.limit.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);
    let filters = query
        .filters
        .iter()
        .map(|f| ctx.filter(f))
        .collect::<Result<Vec<_>, _>>()?;
    let where_sql = if filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filters.join(" AND "))
    };

    let (select, columns) = if query.dimensions.is_empty() && query.measures.is_empty() {
        raw_select(&ctx, &where_sql)
    } else {
        ctx.aggregate_select(&query.dimensions, &query.measures, &where_sql)?
    };

    let order = order_by(query, &columns)?;
    let offset = match query.offset {
        Some(n) if n > 0 => format!(" OFFSET {n}"),
        _ => String::new(),
    };
    Ok(CompiledQuery {
        sql: format!("{select}{order} LIMIT {}{offset}", u64::from(row_limit) + 1),
        params: ctx.params,
        columns,
        row_limit,
    })
}

struct Ctx<'a> {
    table: &'a TableSchema,
    params: Vec<ParamValue>,
}

impl Ctx<'_> {
    fn column(&self, name: &str) -> Result<(&ColumnInfo, TypeClass), PlanError> {
        self.table
            .columns
            .iter()
            .find(|c| c.name == name)
            .map(|c| (c, TypeClass::of(&c.data_type)))
            .ok_or_else(|| PlanError::UnknownColumn(name.to_owned()))
    }

    fn aggregate_select(
        &self,
        dimensions: &[Dimension],
        measures: &[Measure],
        where_sql: &str,
    ) -> Result<(String, Vec<OutputColumn>), PlanError> {
        let mut names = Names::default();
        let mut columns = Vec::new();
        // (inner select items, outer select item) per dimension.
        let mut dims = Vec::new();
        for (i, d) in dimensions.iter().enumerate() {
            let (info, class) = self.column(&d.column)?;
            if d.bucket.is_some() && !class.is_temporal() {
                return Err(PlanError::NotTemporal {
                    column: info.name.clone(),
                    data_type: info.data_type.clone(),
                });
            }
            let alias = names.unique(&match d.bucket {
                Some(b) => format!("{}_{}", info.name, bucket_name(b)),
                None => info.name.clone(),
            });
            dims.push(dimension_parts(
                i,
                &ident(&info.name),
                class,
                d.bucket,
                &alias,
            ));
            columns.push(OutputColumn {
                name: alias,
                role: ColumnRole::Dimension,
            });
        }
        let mut aggs = Vec::new();
        for m in measures {
            let (expr, base) = self.measure(m)?;
            let alias = names.unique(&base);
            aggs.push((expr, alias.clone()));
            columns.push(OutputColumn {
                name: alias,
                role: ColumnRole::Measure,
            });
        }
        let group_by = if dims.is_empty() { "" } else { " GROUP BY ALL" };
        let from = &self.table.reference;

        // Year/quarter/month buckets group on integer date parts (2.7× faster than date_trunc,
        // docs/adr/0004) and rebuild the bucket date over the few aggregated rows.
        let two_level = dims.iter().any(|d| d.outer.is_some());
        let sql = if two_level {
            let inner = dims
                .iter()
                .flat_map(|d| d.inner.iter().cloned())
                .chain(
                    aggs.iter()
                        .enumerate()
                        .map(|(j, (e, _))| format!("{e} AS \"m{j}\"")),
                )
                .collect::<Vec<_>>()
                .join(", ");
            let outer = dims
                .iter()
                .map(|d| d.outer.clone().unwrap_or_else(|| d.simple_outer()))
                .chain(
                    aggs.iter()
                        .enumerate()
                        .map(|(j, (_, a))| format!("\"m{j}\" AS {}", ident(a))),
                )
                .collect::<Vec<_>>()
                .join(", ");
            format!("SELECT {outer} FROM (SELECT {inner} FROM {from}{where_sql}{group_by})")
        } else {
            let items = dims
                .iter()
                .map(|d| format!("{} AS {}", d.expr, ident(&d.alias)))
                .chain(aggs.iter().map(|(e, a)| format!("{e} AS {}", ident(a))))
                .collect::<Vec<_>>()
                .join(", ");
            format!("SELECT {items} FROM {from}{where_sql}{group_by}")
        };
        Ok((sql, columns))
    }

    fn measure(&self, m: &Measure) -> Result<(String, String), PlanError> {
        let name = agg_name(m.agg);
        let Some(column) = &m.column else {
            return match m.agg {
                Aggregation::Count => Ok(("count(*)".into(), "count".into())),
                _ => Err(PlanError::MissingColumn(name)),
            };
        };
        let (info, class) = self.column(column)?;
        let c = ident(&info.name);
        let expr = match m.agg {
            Aggregation::Count => format!("count({c})"),
            Aggregation::Sum | Aggregation::Avg if !class.is_numeric() => {
                return Err(PlanError::NotNumeric {
                    agg: name,
                    column: info.name.clone(),
                    data_type: info.data_type.clone(),
                });
            }
            Aggregation::Sum => format!("sum({c})"),
            Aggregation::Avg => format!("avg({c})"),
            Aggregation::Min => format!("min({c})"),
            Aggregation::Max => format!("max({c})"),
            Aggregation::CountDistinct => format!("count(DISTINCT {c})"),
            Aggregation::ApproxCountDistinct => format!("approx_count_distinct({c})"),
        };
        Ok((expr, format!("{name}_{}", info.name)))
    }

    fn filter(&mut self, f: &Filter) -> Result<String, PlanError> {
        let (info, class) = self.column(&f.column)?;
        let (name, c) = (info.name.clone(), ident(&info.name));
        let invalid = |reason: &str| PlanError::InvalidFilter {
            column: name.clone(),
            reason: reason.to_owned(),
        };
        let comparison = match f.op {
            FilterOp::Eq => Some("="),
            FilterOp::Neq => Some("<>"),
            FilterOp::Gt => Some(">"),
            FilterOp::Gte => Some(">="),
            FilterOp::Lt => Some("<"),
            FilterOp::Lte => Some("<="),
            _ => None,
        };
        if let Some(op) = comparison {
            let Some(FilterValue::One(v)) = &f.value else {
                return Err(invalid("needs a single value"));
            };
            let p = self.bind(v, class, &name)?;
            return Ok(format!("{c} {op} {p}"));
        }
        match f.op {
            FilterOp::IsNull => Ok(format!("{c} IS NULL")),
            FilterOp::IsNotNull => Ok(format!("{c} IS NOT NULL")),
            FilterOp::In | FilterOp::NotIn => {
                let Some(FilterValue::Many(values)) = &f.value else {
                    return Err(invalid("needs a list of values"));
                };
                if values.is_empty() || values.len() > 1000 {
                    return Err(invalid("needs between 1 and 1000 values"));
                }
                let placeholders = values
                    .iter()
                    .map(|v| self.bind(v, class, &name))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                let not = if f.op == FilterOp::NotIn { "NOT " } else { "" };
                Ok(format!("{c} {not}IN ({placeholders})"))
            }
            FilterOp::Between => match &f.value {
                Some(FilterValue::Many(v)) if v.len() == 2 => {
                    let lo = self.bind(&v[0], class, &name)?;
                    let hi = self.bind(&v[1], class, &name)?;
                    Ok(format!("{c} BETWEEN {lo} AND {hi}"))
                }
                _ => Err(invalid("needs exactly two values")),
            },
            FilterOp::Contains => match &f.value {
                Some(FilterValue::One(Scalar::Text(s))) => {
                    self.params.push(ParamValue::Text(s.clone()));
                    Ok(format!("contains(lower(CAST({c} AS VARCHAR)), lower(?))"))
                }
                _ => Err(invalid("needs a text value")),
            },
            _ => Err(invalid("unsupported operator")),
        }
    }

    /// Binds a scalar for comparison with a column of `class`, returning its placeholder.
    fn bind(
        &mut self,
        value: &Scalar,
        class: TypeClass,
        column: &str,
    ) -> Result<String, PlanError> {
        let param = match (value, class) {
            (Scalar::Number(n), TypeClass::Integer)
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 =>
            {
                ParamValue::BigInt(*n as i64)
            }
            (Scalar::Number(n), c) if c.is_numeric() && n.is_finite() => ParamValue::Double(*n),
            (Scalar::Bool(b), TypeClass::Boolean) => ParamValue::Bool(*b),
            (Scalar::Text(s), c) if c.is_temporal() => {
                self.params.push(ParamValue::Text(s.clone()));
                let target = c.cast_target().unwrap_or("VARCHAR");
                return Ok(format!("CAST(? AS {target})"));
            }
            (Scalar::Text(s), TypeClass::Text | TypeClass::Other) => ParamValue::Text(s.clone()),
            _ => {
                return Err(PlanError::InvalidFilter {
                    column: column.to_owned(),
                    reason: "value does not match the column type".into(),
                });
            }
        };
        self.params.push(param);
        Ok("?".into())
    }
}

fn raw_select(ctx: &Ctx<'_>, where_sql: &str) -> (String, Vec<OutputColumn>) {
    let columns: Vec<OutputColumn> = ctx
        .table
        .columns
        .iter()
        .map(|c| OutputColumn {
            name: c.name.clone(),
            role: ColumnRole::Column,
        })
        .collect();
    let list = ctx
        .table
        .columns
        .iter()
        .map(|c| ident(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!("SELECT {list} FROM {}{where_sql}", ctx.table.reference),
        columns,
    )
}

fn order_by(query: &TableQuery, columns: &[OutputColumn]) -> Result<String, PlanError> {
    let items: Vec<String> = if query.sort.is_empty() {
        // Deterministic default: dimensions in order (aggregation output order is otherwise arbitrary).
        columns
            .iter()
            .filter(|c| c.role == ColumnRole::Dimension)
            .map(|c| format!("{} ASC NULLS LAST", ident(&c.name)))
            .collect()
    } else {
        query
            .sort
            .iter()
            .map(|s| {
                if !columns.iter().any(|c| c.name == s.by) {
                    return Err(PlanError::UnknownSort(s.by.clone()));
                }
                let dir = match s.dir {
                    SortDir::Asc => "ASC",
                    SortDir::Desc => "DESC",
                };
                Ok(format!("{} {dir} NULLS LAST", ident(&s.by)))
            })
            .collect::<Result<_, _>>()?
    };
    Ok(if items.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", items.join(", "))
    })
}

struct DimParts {
    /// Expression for the single-level form.
    expr: String,
    alias: String,
    /// Select items for the inner query of the two-level form.
    inner: Vec<String>,
    /// Outer select item when the dimension needs rebuilding from integer parts.
    outer: Option<String>,
    key: String,
}

impl DimParts {
    fn simple_outer(&self) -> String {
        format!("{} AS {}", ident(&self.key), ident(&self.alias))
    }
}

fn dimension_parts(
    i: usize,
    c: &str,
    class: TypeClass,
    bucket: Option<TimeBucket>,
    alias: &str,
) -> DimParts {
    let key = format!("k{i}");
    let (y, q, m) = (format!("k{i}_y"), format!("k{i}_q"), format!("k{i}_m"));
    let a = ident(alias);
    let simple = |expr: String| DimParts {
        inner: vec![format!("{expr} AS {}", ident(&key))],
        expr,
        alias: alias.to_owned(),
        outer: None,
        key: key.clone(),
    };
    match bucket {
        None => simple(c.to_owned()),
        Some(TimeBucket::Day) if class == TypeClass::Date => simple(c.to_owned()),
        Some(TimeBucket::Day) => simple(format!("CAST({c} AS DATE)")),
        Some(TimeBucket::Week) => simple(format!("CAST(date_trunc('week', {c}) AS DATE)")),
        Some(TimeBucket::Year) => DimParts {
            expr: format!("make_date(year({c}), 1, 1)"),
            alias: alias.to_owned(),
            inner: vec![format!("year({c}) AS {}", ident(&y))],
            outer: Some(format!("make_date({}, 1, 1) AS {a}", ident(&y))),
            key,
        },
        Some(TimeBucket::Quarter) => DimParts {
            expr: format!("make_date(year({c}), (quarter({c}) - 1) * 3 + 1, 1)"),
            alias: alias.to_owned(),
            inner: vec![
                format!("year({c}) AS {}", ident(&y)),
                format!("quarter({c}) AS {}", ident(&q)),
            ],
            outer: Some(format!(
                "make_date({}, ({} - 1) * 3 + 1, 1) AS {a}",
                ident(&y),
                ident(&q)
            )),
            key,
        },
        Some(TimeBucket::Month) => DimParts {
            expr: format!("make_date(year({c}), month({c}), 1)"),
            alias: alias.to_owned(),
            inner: vec![
                format!("year({c}) AS {}", ident(&y)),
                format!("month({c}) AS {}", ident(&m)),
            ],
            outer: Some(format!("make_date({}, {}, 1) AS {a}", ident(&y), ident(&m))),
            key,
        },
    }
}

fn bucket_name(b: TimeBucket) -> &'static str {
    match b {
        TimeBucket::Year => "year",
        TimeBucket::Quarter => "quarter",
        TimeBucket::Month => "month",
        TimeBucket::Week => "week",
        TimeBucket::Day => "day",
    }
}

fn agg_name(a: Aggregation) -> &'static str {
    match a {
        Aggregation::Count => "count",
        Aggregation::Sum => "sum",
        Aggregation::Avg => "avg",
        Aggregation::Min => "min",
        Aggregation::Max => "max",
        Aggregation::CountDistinct => "count_distinct",
        Aggregation::ApproxCountDistinct => "approx_count_distinct",
    }
}

#[derive(Default)]
struct Names(Vec<String>);

impl Names {
    fn unique(&mut self, base: &str) -> String {
        let mut name = base.to_owned();
        let mut n = 2;
        while self.0.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        self.0.push(name.clone());
        name
    }
}
