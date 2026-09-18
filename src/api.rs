use crate::MAX_UPLOAD_BYTES;
use crate::chat::{self, ChatIn};
use crate::donors::DonorQueue;
use crate::encoding::{self, EncodingReport};
use crate::engine::{self, CompareReport, ConversionStats};
use crate::error::AppError;
use crate::status;
use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, Multipart, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_core::Stream;
use metrics::{counter, histogram};
use metrics_exporter_prometheus::PrometheusHandle;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

#[derive(Clone)]
pub struct AppState {
    pub metrics: PrometheusHandle,
    pub donors: Arc<DonorQueue>,
    pub cpu: Arc<status::CpuSampler>,
    pub llm_url: String,
    pub http: reqwest::Client,
}

pub async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "service": "datacooking.dev",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "max_upload_bytes": MAX_UPLOAD_BYTES,
        "tools": [
            { "id": "fix-encoding", "path": "/fix-encoding.html", "status": "live" },
            { "id": "csv-parquet", "path": "/csv-parquet.html", "status": "live" },
            { "id": "my-ip", "path": "/my-ip.html", "status": "live" },
            { "id": "subnet", "path": "/subnet.html", "status": "live" },
            { "id": "chat", "path": "/chat.html", "status": "live" }
        ],
    }))
}

pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    state.metrics.render()
}

pub async fn status_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    Json(json!({
        "cpu_percent": state.cpu.percent(),
        "ram": status::ram(),
        "llm_ready": chat::llm_ready(&state.llm_url, &state.http).await,
        "ip": client_ip(&headers, addr),
    }))
}

pub async fn chat_handler(
    State(state): State<AppState>,
    Json(body): Json<ChatIn>,
) -> Result<Response, AppError> {
    chat::proxy_chat(&state.llm_url, &state.http, body).await
}

pub async fn my_ip_handler(
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    Json(json!({
        "ip": client_ip(&headers, addr),
    }))
}

fn client_ip(headers: &HeaderMap, addr: SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.ip().to_string())
}

pub async fn donors_list(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.donors.list())
}

#[derive(Deserialize)]
pub struct DonorIn {
    name: String,
    #[serde(default)]
    note: String,
}

pub async fn donors_add(
    State(state): State<AppState>,
    Json(body): Json<DonorIn>,
) -> Result<impl IntoResponse, AppError> {
    let donor = state
        .donors
        .push(&body.name, &body.note)
        .map_err(AppError::bad_request)?;
    Ok(Json(donor))
}

pub async fn csv_to_parquet_handler(multipart: Multipart) -> Result<Response, AppError> {
    let started = Instant::now();
    let upload = save_upload(multipart, "csv").await?;
    let parquet = NamedTempFile::new().map_err(|e| AppError::internal(e.to_string()))?;

    let stats = convert_blocking(
        "csv_to_parquet",
        upload.temp.path().to_path_buf(),
        parquet.path().to_path_buf(),
        engine::csv_to_parquet,
    )
    .await?;

    let elapsed = started.elapsed();
    record_success("csv_to_parquet", &stats, elapsed.as_secs_f64());

    let filename = replace_ext(&upload.original_name, ".csv", ".parquet");
    file_response(
        parquet,
        "application/vnd.apache.parquet",
        &filename,
        &stats,
        elapsed.as_millis(),
    )
    .await
}

pub async fn parquet_to_csv_handler(multipart: Multipart) -> Result<Response, AppError> {
    let started = Instant::now();
    let upload = save_upload(multipart, "parquet").await?;
    let csv = NamedTempFile::new().map_err(|e| AppError::internal(e.to_string()))?;

    let stats = convert_blocking(
        "parquet_to_csv",
        upload.temp.path().to_path_buf(),
        csv.path().to_path_buf(),
        engine::parquet_to_csv,
    )
    .await?;

    let elapsed = started.elapsed();
    record_success("parquet_to_csv", &stats, elapsed.as_secs_f64());

    let filename = replace_ext(&upload.original_name, ".parquet", ".csv");
    file_response(
        csv,
        "text/csv; charset=utf-8",
        &filename,
        &stats,
        elapsed.as_millis(),
    )
    .await
}

pub async fn compare_handler(multipart: Multipart) -> Result<Json<CompareJson>, AppError> {
    let started = Instant::now();
    let upload = save_upload(multipart, "csv").await?;
    let path = upload.temp.path().to_path_buf();
    let report = tokio::task::spawn_blocking(move || engine::compare_csv_roundtrip(&path))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(|e| {
            record_failure("compare");
            AppError::unprocessable(format!("Compare failed: {e}"))
        })?;

    let elapsed = started.elapsed();
    counter!("datacooking_conversions_total", "direction" => "compare", "status" => "success")
        .increment(1);
    histogram!("datacooking_conversion_duration_seconds", "direction" => "compare")
        .record(elapsed.as_secs_f64());

    tracing::info!(
        direction = "compare",
        rows = report.rows,
        columns = report.columns,
        csv_bytes = report.csv_bytes,
        parquet_bytes = report.parquet_bytes,
        round_trip_ok = report.round_trip_ok,
        elapsed_ms = elapsed.as_millis() as u64,
        "CSV ↔ Parquet compare completed"
    );

    Ok(Json(CompareJson {
        duration_ms: elapsed.as_millis(),
        report,
    }))
}

pub async fn encoding_preview_handler(
    multipart: Multipart,
) -> Result<Json<EncodingPreview>, AppError> {
    let upload = save_upload(multipart, "text").await?;
    let path = upload.temp.path().to_path_buf();
    let report = tokio::task::spawn_blocking(move || encoding::inspect(&path))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(|e| {
            record_failure("fix_encoding");
            AppError::unprocessable(format!("inspect failed: {e}"))
        })?;
    Ok(Json(EncodingPreview { report }))
}

pub async fn encoding_fix_handler(multipart: Multipart) -> Result<Response, AppError> {
    let started = Instant::now();
    let upload = save_upload(multipart, "text").await?;
    let output = NamedTempFile::new().map_err(|e| AppError::internal(e.to_string()))?;
    let input_path = upload.temp.path().to_path_buf();
    let output_path = output.path().to_path_buf();
    let report = tokio::task::spawn_blocking(move || encoding::convert(&input_path, &output_path))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(|e| {
            record_failure("fix_encoding");
            AppError::unprocessable(format!("fix failed: {e}"))
        })?;

    let elapsed = started.elapsed();
    counter!("datacooking_conversions_total", "direction" => "fix_encoding", "status" => "success")
        .increment(1);
    histogram!("datacooking_conversion_duration_seconds", "direction" => "fix_encoding")
        .record(elapsed.as_secs_f64());

    let filename = Path::new(&upload.original_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("fixed.csv");

    let out_bytes = std::fs::metadata(output.path())
        .map_err(|e| AppError::internal(e.to_string()))?
        .len() as usize;
    encoding_file_response(output, &filename, &report, out_bytes, elapsed.as_millis()).await
}

#[derive(Serialize)]
pub struct EncodingPreview {
    #[serde(flatten)]
    pub report: EncodingReport,
}

#[derive(Serialize)]
pub struct CompareJson {
    pub duration_ms: u128,
    #[serde(flatten)]
    pub report: CompareReport,
}

struct Upload {
    temp: NamedTempFile,
    original_name: String,
}

struct GuardedFileStream {
    _keep_alive: NamedTempFile,
    inner: ReaderStream<tokio::fs::File>,
}

impl Stream for GuardedFileStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

async fn convert_blocking(
    direction: &'static str,
    input: PathBuf,
    output: PathBuf,
    work: fn(&Path, &Path) -> engine::EngineResult<ConversionStats>,
) -> Result<ConversionStats, AppError> {
    tokio::task::spawn_blocking(move || work(&input, &output))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(|e| {
            record_failure(direction);
            AppError::unprocessable(format!("{direction} failed: {e}"))
        })
}

async fn save_upload(mut multipart: Multipart, kind: &str) -> Result<Upload, AppError> {
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }

        let original_name = field.file_name().unwrap_or("upload").to_string();
        if !extension_ok(&original_name, kind) {
            return Err(AppError::bad_request(match kind {
                "csv" => "expected a .csv file",
                "text" => "expected a .csv, .txt, or .tsv file",
                _ => "expected a .parquet file",
            }));
        }

        let temp = NamedTempFile::new().map_err(|e| AppError::internal(e.to_string()))?;
        let mut out = tokio::fs::File::from_std(
            temp.reopen()
                .map_err(|e| AppError::internal(e.to_string()))?,
        );

        let mut written = 0usize;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| AppError::bad_request(e.to_string()))?
        {
            written = written
                .checked_add(chunk.len())
                .ok_or_else(|| AppError::bad_request("file is too large"))?;
            if written > MAX_UPLOAD_BYTES {
                return Err(AppError::bad_request(format!(
                    "file exceeds {} MB limit",
                    MAX_UPLOAD_BYTES / 1024 / 1024
                )));
            }
            out.write_all(&chunk)
                .await
                .map_err(|e| AppError::internal(e.to_string()))?;
        }
        out.flush()
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;

        if written == 0 {
            return Err(AppError::bad_request("file is empty"));
        }

        return Ok(Upload {
            temp,
            original_name,
        });
    }

    Err(AppError::bad_request("missing multipart field \"file\""))
}

fn extension_ok(name: &str, kind: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match kind {
        "csv" => ext == "csv",
        "text" => matches!(ext.as_str(), "csv" | "txt" | "tsv"),
        "parquet" => ext == "parquet",
        _ => false,
    }
}

fn replace_ext(name: &str, from: &str, to: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let lower = base.to_ascii_lowercase();
    let from_lower = from.to_ascii_lowercase();
    let stem = if let Some(stripped) = lower.strip_suffix(&from_lower) {
        &base[..stripped.len()]
    } else {
        base
    };
    let stem = if stem.is_empty() { "converted" } else { stem };
    format!("{stem}{to}")
}

fn ascii_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() {
        "download.bin".into()
    } else {
        cleaned
    }
}

async fn file_response(
    temp: NamedTempFile,
    content_type: &'static str,
    filename: &str,
    stats: &ConversionStats,
    duration_ms: u128,
) -> Result<Response, AppError> {
    let file = tokio::fs::File::open(temp.path())
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    let filename = ascii_filename(filename);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    headers.insert(header::CONTENT_LENGTH, header_usize(stats.out_bytes)?);
    headers.insert("X-Rows", header_usize(stats.rows)?);
    headers.insert("X-Columns", header_usize(stats.columns)?);
    headers.insert("X-Original-Bytes", header_usize(stats.in_bytes)?);
    headers.insert("X-Output-Bytes", header_usize(stats.out_bytes)?);
    headers.insert(
        "X-Reduction-Percent",
        HeaderValue::from_str(&format!("{:.1}", stats.reduction_percent))
            .map_err(|e| AppError::internal(e.to_string()))?,
    );
    headers.insert("X-Duration-Ms", header_usize(duration_ms as usize)?);

    if let Ok(json) = serde_json::to_string(&stats.schema) {
        if json.is_ascii() && json.len() < 6_000 {
            if let Ok(value) = HeaderValue::from_str(&json) {
                headers.insert("X-Schema", value);
            }
        }
    }

    let body = Body::from_stream(GuardedFileStream {
        _keep_alive: temp,
        inner: ReaderStream::new(file),
    });
    Ok((headers, body).into_response())
}

async fn encoding_file_response(
    temp: NamedTempFile,
    filename: &str,
    report: &EncodingReport,
    out_bytes: usize,
    duration_ms: u128,
) -> Result<Response, AppError> {
    let file = tokio::fs::File::open(temp.path())
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;
    let filename = ascii_filename(filename);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    headers.insert(header::CONTENT_LENGTH, header_usize(out_bytes)?);
    headers.insert("X-Detected-Encoding", header_str(&report.encoding)?);
    headers.insert("X-Thai-Chars", header_usize(report.thai_chars)?);
    headers.insert("X-Confidence", header_str(&report.confidence)?);
    headers.insert(
        "X-Already-Ok",
        header_str(if report.already_ok { "1" } else { "0" })?,
    );
    headers.insert("X-Duration-Ms", header_usize(duration_ms as usize)?);

    let body = Body::from_stream(GuardedFileStream {
        _keep_alive: temp,
        inner: ReaderStream::new(file),
    });
    Ok((headers, body).into_response())
}

fn header_str(value: &str) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(value).map_err(|e| AppError::internal(e.to_string()))
}

fn header_usize(value: usize) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(&value.to_string()).map_err(|e| AppError::internal(e.to_string()))
}

fn record_success(direction: &'static str, stats: &ConversionStats, seconds: f64) {
    counter!("datacooking_conversions_total", "direction" => direction, "status" => "success")
        .increment(1);
    counter!("datacooking_bytes_in_total", "direction" => direction)
        .increment(stats.in_bytes as u64);
    counter!("datacooking_bytes_out_total", "direction" => direction)
        .increment(stats.out_bytes as u64);
    histogram!("datacooking_conversion_duration_seconds", "direction" => direction).record(seconds);

    tracing::info!(
        direction,
        rows = stats.rows,
        columns = stats.columns,
        in_bytes = stats.in_bytes,
        out_bytes = stats.out_bytes,
        reduction_pct = format!("{:.1}%", stats.reduction_percent),
        elapsed_ms = (seconds * 1000.0) as u64,
        "Conversion completed"
    );
}

fn record_failure(direction: &'static str) {
    counter!("datacooking_conversions_total", "direction" => direction, "status" => "error")
        .increment(1);
}
