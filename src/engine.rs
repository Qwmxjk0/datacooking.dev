use arrow::csv::reader::infer_schema_from_files;
use arrow::csv::{ReaderBuilder, WriterBuilder};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::sync::Arc;

const IO_BUF: usize = 256 * 1024;
const BATCH_SIZE: usize = 65_536;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversionStats {
    pub rows: usize,
    pub columns: usize,
    pub in_bytes: usize,
    pub out_bytes: usize,
    pub reduction_percent: f64,
    pub schema: Vec<ColumnMeta>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareReport {
    pub rows: usize,
    pub columns: usize,
    pub csv_bytes: usize,
    pub parquet_bytes: usize,
    pub restored_csv_bytes: usize,
    pub reduction_percent: f64,
    pub round_trip_ok: bool,
    pub rows_match: bool,
    pub column_names_match: bool,
    pub schema: Vec<ColumnMeta>,
    pub restored_schema: Vec<ColumnMeta>,
}

pub type EngineResult<T> = Result<T, String>;

fn reduction_percent(in_bytes: usize, out_bytes: usize) -> f64 {
    if in_bytes == 0 {
        0.0
    } else {
        100.0 - (out_bytes as f64 / in_bytes as f64 * 100.0)
    }
}

fn column_meta(schema: &arrow::datatypes::Schema) -> Vec<ColumnMeta> {
    schema
        .fields()
        .iter()
        .map(|field| ColumnMeta {
            name: field.name().to_string(),
            data_type: field.data_type().to_string(),
        })
        .collect()
}

fn path_as_utf8(path: &Path, label: &str) -> EngineResult<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{label} path is not valid UTF-8"))
}

/// Convert CSV (header required, comma-separated) to ZSTD-compressed Parquet.
pub fn csv_to_parquet(csv_path: &Path, parquet_path: &Path) -> EngineResult<ConversionStats> {
    let csv_path_str = path_as_utf8(csv_path, "CSV")?;
    let schema = infer_schema_from_files(&[csv_path_str], b',', Some(100), true)
        .map_err(|e| format!("failed to infer CSV schema: {e}"))?;
    let schema_meta = column_meta(&schema);
    let columns = schema_meta.len();
    let schema_ref = Arc::new(schema);

    let file = File::open(csv_path).map_err(|e| format!("failed to open CSV: {e}"))?;
    let csv_reader = ReaderBuilder::new(schema_ref.clone())
        .with_header(true)
        .with_batch_size(BATCH_SIZE)
        .build(BufReader::with_capacity(IO_BUF, file))
        .map_err(|e| format!("failed to read CSV: {e}"))?;

    let out_file = BufWriter::with_capacity(
        IO_BUF,
        File::create(parquet_path).map_err(|e| format!("failed to create Parquet: {e}"))?,
    );
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .set_dictionary_enabled(true)
        .build();
    let mut writer = ArrowWriter::try_new(out_file, schema_ref, Some(props))
        .map_err(|e| format!("failed to start Parquet writer: {e}"))?;

    let mut rows = 0usize;
    for batch in csv_reader {
        let record_batch = batch.map_err(|e| format!("failed to parse CSV batch: {e}"))?;
        rows += record_batch.num_rows();
        writer
            .write(&record_batch)
            .map_err(|e| format!("failed to write Parquet batch: {e}"))?;
    }
    writer
        .close()
        .map_err(|e| format!("failed to close Parquet writer: {e}"))?;

    let in_bytes = std::fs::metadata(csv_path)
        .map_err(|e| format!("failed to stat CSV: {e}"))?
        .len() as usize;
    let out_bytes = std::fs::metadata(parquet_path)
        .map_err(|e| format!("failed to stat Parquet: {e}"))?
        .len() as usize;

    Ok(ConversionStats {
        rows,
        columns,
        in_bytes,
        out_bytes,
        reduction_percent: reduction_percent(in_bytes, out_bytes),
        schema: schema_meta,
    })
}

/// Convert Parquet back to CSV with a header row.
pub fn parquet_to_csv(parquet_path: &Path, csv_path: &Path) -> EngineResult<ConversionStats> {
    let file = File::open(parquet_path).map_err(|e| format!("failed to open Parquet: {e}"))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| format!("failed to read Parquet: {e}"))?
        .with_batch_size(BATCH_SIZE);
    let schema_meta = column_meta(builder.schema().as_ref());
    let columns = schema_meta.len();
    let reader = builder
        .build()
        .map_err(|e| format!("failed to build Parquet reader: {e}"))?;

    let out_file = BufWriter::with_capacity(
        IO_BUF,
        File::create(csv_path).map_err(|e| format!("failed to create CSV: {e}"))?,
    );
    let mut writer = WriterBuilder::new().with_header(true).build(out_file);

    let mut rows = 0usize;
    for batch in reader {
        let record_batch = batch.map_err(|e| format!("failed to read Parquet batch: {e}"))?;
        rows += record_batch.num_rows();
        writer
            .write(&record_batch)
            .map_err(|e| format!("failed to write CSV batch: {e}"))?;
    }

    let in_bytes = std::fs::metadata(parquet_path)
        .map_err(|e| format!("failed to stat Parquet: {e}"))?
        .len() as usize;
    let out_bytes = std::fs::metadata(csv_path)
        .map_err(|e| format!("failed to stat CSV: {e}"))?
        .len() as usize;

    Ok(ConversionStats {
        rows,
        columns,
        in_bytes,
        out_bytes,
        reduction_percent: reduction_percent(in_bytes, out_bytes),
        schema: schema_meta,
    })
}

/// CSV → Parquet → CSV: size compare plus row/column round-trip check.
pub fn compare_csv_roundtrip(csv_path: &Path) -> EngineResult<CompareReport> {
    let parquet = tempfile::NamedTempFile::new()
        .map_err(|e| format!("failed to create temp Parquet: {e}"))?;
    let restored =
        tempfile::NamedTempFile::new().map_err(|e| format!("failed to create temp CSV: {e}"))?;

    let to_parquet = csv_to_parquet(csv_path, parquet.path())?;
    let to_csv = parquet_to_csv(parquet.path(), restored.path())?;

    let original_names: Vec<&str> = to_parquet.schema.iter().map(|c| c.name.as_str()).collect();
    let restored_names: Vec<&str> = to_csv.schema.iter().map(|c| c.name.as_str()).collect();
    let rows_match = to_parquet.rows == to_csv.rows;
    let column_names_match = original_names == restored_names;

    Ok(CompareReport {
        rows: to_parquet.rows,
        columns: to_parquet.columns,
        csv_bytes: to_parquet.in_bytes,
        parquet_bytes: to_parquet.out_bytes,
        restored_csv_bytes: to_csv.out_bytes,
        reduction_percent: to_parquet.reduction_percent,
        round_trip_ok: rows_match && column_names_match,
        rows_match,
        column_names_match,
        schema: to_parquet.schema,
        restored_schema: to_csv.schema,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_csv(path: &Path, rows: usize) {
        let mut file = File::create(path).unwrap();
        writeln!(file, "id,name,city,amount").unwrap();
        for i in 0..rows {
            writeln!(file, "{i},user{},Bangkok,{:.2}", i % 10, i as f64 * 1.5).unwrap();
        }
    }

    #[test]
    fn csv_to_parquet_counts_rows_and_columns() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("sample.csv");
        let parquet = dir.path().join("sample.parquet");
        write_csv(&csv, 25);

        let stats = csv_to_parquet(&csv, &parquet).unwrap();
        assert_eq!(stats.rows, 25);
        assert_eq!(stats.columns, 4);
        assert!(parquet.exists());
        assert!(stats.out_bytes > 0);
        assert_eq!(
            stats
                .schema
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["id", "name", "city", "amount"]
        );
    }

    #[test]
    fn roundtrip_preserves_rows_and_column_names() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("sample.csv");
        write_csv(&csv, 40);

        let report = compare_csv_roundtrip(&csv).unwrap();
        assert!(report.round_trip_ok);
        assert!(report.rows_match);
        assert!(report.column_names_match);
        assert_eq!(report.rows, 40);
        assert_eq!(report.columns, 4);
        assert!(report.parquet_bytes > 0);
        assert!(report.restored_csv_bytes > 0);
    }

    #[test]
    fn repeated_values_compress_smaller_than_csv() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("sample.csv");
        let parquet = dir.path().join("sample.parquet");
        write_csv(&csv, 2_000);

        let stats = csv_to_parquet(&csv, &parquet).unwrap();
        assert_eq!(stats.rows, 2_000);
        assert!(
            stats.out_bytes < stats.in_bytes,
            "expected parquet {} < csv {}",
            stats.out_bytes,
            stats.in_bytes
        );
        assert!(stats.reduction_percent > 0.0);
    }

    #[test]
    fn invalid_parquet_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("not.parquet");
        let csv = dir.path().join("out.csv");
        std::fs::write(&bogus, b"this is not parquet").unwrap();
        assert!(parquet_to_csv(&bogus, &csv).is_err());
    }
}
