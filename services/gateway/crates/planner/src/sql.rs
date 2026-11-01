//! SQL text helpers. Every identifier that reaches SQL goes through [`ident`].

/// Quotes an identifier for DuckDB: wraps in double quotes and doubles embedded quotes.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Broad type classes, from DuckDB's type names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    Integer,
    Float,
    Decimal,
    Boolean,
    Text,
    Date,
    Timestamp,
    TimestampTz,
    Other,
}

impl TypeClass {
    pub fn of(data_type: &str) -> Self {
        let t = data_type.trim().to_ascii_uppercase();
        let base = t.split('(').next().unwrap_or("").trim();
        match base {
            "TINYINT" | "SMALLINT" | "INTEGER" | "INT" | "BIGINT" | "HUGEINT" | "UTINYINT"
            | "USMALLINT" | "UINTEGER" | "UBIGINT" | "UHUGEINT" | "INT1" | "INT2" | "INT4"
            | "INT8" | "LONG" | "SHORT" => TypeClass::Integer,
            "FLOAT" | "REAL" | "DOUBLE" | "FLOAT4" | "FLOAT8" => TypeClass::Float,
            "DECIMAL" | "NUMERIC" => TypeClass::Decimal,
            "BOOLEAN" | "BOOL" => TypeClass::Boolean,
            "VARCHAR" | "TEXT" | "STRING" | "CHAR" | "BPCHAR" | "UUID" => TypeClass::Text,
            "DATE" => TypeClass::Date,
            "TIMESTAMP WITH TIME ZONE" | "TIMESTAMPTZ" => TypeClass::TimestampTz,
            "TIMESTAMP" | "DATETIME" | "TIMESTAMP_S" | "TIMESTAMP_MS" | "TIMESTAMP_NS"
            | "TIMESTAMP_US" => TypeClass::Timestamp,
            _ => TypeClass::Other,
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            TypeClass::Integer | TypeClass::Float | TypeClass::Decimal
        )
    }

    pub fn is_temporal(self) -> bool {
        matches!(
            self,
            TypeClass::Date | TypeClass::Timestamp | TypeClass::TimestampTz
        )
    }

    /// Canonical cast target for binding ISO strings against temporal columns.
    pub fn cast_target(self) -> Option<&'static str> {
        match self {
            TypeClass::Date => Some("DATE"),
            TypeClass::Timestamp => Some("TIMESTAMP"),
            TypeClass::TimestampTz => Some("TIMESTAMPTZ"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_safely() {
        assert_eq!(ident("amount"), "\"amount\"");
        assert_eq!(ident("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(
            ident("x\"; DROP TABLE t; --"),
            "\"x\"\"; DROP TABLE t; --\""
        );
    }

    #[test]
    fn classifies_types() {
        assert_eq!(TypeClass::of("DECIMAL(12,2)"), TypeClass::Decimal);
        assert_eq!(TypeClass::of("bigint"), TypeClass::Integer);
        assert_eq!(
            TypeClass::of("TIMESTAMP WITH TIME ZONE"),
            TypeClass::TimestampTz
        );
        assert_eq!(TypeClass::of("VARCHAR"), TypeClass::Text);
        assert_eq!(TypeClass::of("STRUCT(a INT)"), TypeClass::Other);
    }
}
