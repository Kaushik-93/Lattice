//! Arrow IPC framing for results sent to the browser.

use std::sync::Arc;

use arrow::compute::cast;
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;

use crate::EngineError;

/// Encodes batches as an Arrow IPC stream (what `apache-arrow`'s `tableFromIPC` reads).
/// Uncompressed on purpose: Arrow JS can't decompress IPC buffers, and results are aggregates.
pub fn encode(schema: &SchemaRef, batches: &[RecordBatch]) -> Result<Vec<u8>, EngineError> {
    let mut buf = Vec::with_capacity(
        batches
            .iter()
            .map(RecordBatch::get_array_memory_size)
            .sum::<usize>()
            + 1024,
    );
    let mut writer = StreamWriter::try_new(&mut buf, schema)?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.finish()?;
    drop(writer);
    Ok(buf)
}

/// Keeps the first `limit` rows (results fetch one extra row to detect truncation).
pub fn truncate(batches: Vec<RecordBatch>, limit: usize) -> Vec<RecordBatch> {
    let mut remaining = limit;
    let mut out = Vec::with_capacity(batches.len());
    for batch in batches {
        if remaining == 0 {
            break;
        }
        if batch.num_rows() <= remaining {
            remaining -= batch.num_rows();
            out.push(batch);
        } else {
            out.push(batch.slice(0, remaining));
            remaining = 0;
        }
    }
    out
}

fn is_decimal(t: &DataType) -> bool {
    matches!(t, DataType::Decimal128(..) | DataType::Decimal256(..))
}

/// Arrow JS returns decimals as raw big-number words (no scale, no arithmetic), and charts need
/// numbers, so decimal columns go to the browser as Float64. DuckDB emits DECIMAL and the
/// HUGEINT results of integer sums as Decimal128. Exact values remain available server-side.
pub fn for_browser(
    schema: &SchemaRef,
    batches: Vec<RecordBatch>,
) -> Result<(SchemaRef, Vec<RecordBatch>), EngineError> {
    if !schema.fields().iter().any(|f| is_decimal(f.data_type())) {
        return Ok((schema.clone(), batches));
    }
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .map(|f| {
            if is_decimal(f.data_type()) {
                Field::new(f.name(), DataType::Float64, f.is_nullable())
            } else {
                f.as_ref().clone()
            }
        })
        .collect();
    let target = Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()));
    let batches = batches
        .into_iter()
        .map(|batch| {
            let columns = batch
                .columns()
                .iter()
                .zip(target.fields())
                .map(|(column, field)| {
                    if column.data_type() == field.data_type() {
                        Ok(column.clone())
                    } else {
                        cast(column, field.data_type())
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            RecordBatch::try_new(target.clone(), columns)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((target, batches))
}

#[cfg(test)]
mod tests {
    use arrow::array::{Array, Decimal128Array, Float64Array, Int64Array};

    use super::*;

    #[test]
    fn decimals_become_doubles_for_the_browser() {
        let amount = Decimal128Array::from(vec![Some(1050), None, Some(-325)])
            .with_precision_and_scale(12, 2)
            .unwrap();
        let count = Int64Array::from(vec![1, 2, 3]);
        let schema = Arc::new(Schema::new(vec![
            Field::new("amount", amount.data_type().clone(), true),
            Field::new("count", DataType::Int64, false),
        ]));
        let batch =
            RecordBatch::try_new(schema.clone(), vec![Arc::new(amount), Arc::new(count)]).unwrap();
        let (schema, batches) = for_browser(&schema, vec![batch]).unwrap();
        assert_eq!(schema.field(0).data_type(), &DataType::Float64);
        assert_eq!(
            schema.field(1).data_type(),
            &DataType::Int64,
            "integers are left alone"
        );
        let values = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert_eq!(
            (values.value(0), values.is_null(1), values.value(2)),
            (10.5, true, -3.25)
        );
        assert!(!encode(&schema, &batches).unwrap().is_empty());
    }

    #[test]
    fn truncation_keeps_the_first_rows_across_batches() {
        let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int64, false)]));
        let batch = |v: Vec<i64>| {
            RecordBatch::try_new(schema.clone(), vec![Arc::new(Int64Array::from(v))]).unwrap()
        };
        let out = truncate(vec![batch(vec![1, 2]), batch(vec![3, 4, 5])], 3);
        assert_eq!(out.iter().map(RecordBatch::num_rows).sum::<usize>(), 3);
        assert_eq!(out.len(), 2);
    }
}
