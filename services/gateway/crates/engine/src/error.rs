#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("query cancelled")]
    Cancelled,
    #[error("query exceeded the {0:?} time limit")]
    Timeout(std::time::Duration),
    #[error("the project database is busy; try again")]
    Busy,
    #[error("{0}")]
    Query(String),
    #[error("unusable path {0}")]
    Path(String),
    #[error("could not read the spreadsheet: {0}")]
    Spreadsheet(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("engine task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Arrow(#[from] arrow::error::ArrowError),
}

impl From<duckdb::Error> for EngineError {
    fn from(err: duckdb::Error) -> Self {
        // DuckDB messages are user-meaningful ("Conversion Error: ..."); keep them.
        EngineError::Query(err.to_string())
    }
}
