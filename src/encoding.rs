use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, WINDOWS_874};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

const SAMPLE_BYTES: usize = 64 * 1024;
const PREVIEW_CHARS: usize = 500;
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    Utf8AddBom,
    Decode(&'static Encoding),
    Mojibake,
}

#[derive(Debug, Clone, Serialize)]
pub struct EncodingReport {
    pub encoding: String,
    pub already_ok: bool,
    pub thai_chars: usize,
    pub preview: String,
    pub confidence: String,
}

pub type EncodeResult<T> = Result<T, String>;

fn thai_count(s: &str) -> usize {
    s.chars()
        .filter(|c| ('\u{0E00}'..='\u{0E7F}').contains(c))
        .count()
}

fn clip_preview(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= PREVIEW_CHARS {
            out.push('…');
            break;
        }
        if c == '\u{FFFD}' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn try_mojibake(s: &str) -> Option<String> {
    if thai_count(s) > 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if u > 255 {
            return None;
        }
        bytes.push(u as u8);
    }
    let fixed = std::str::from_utf8(&bytes).ok()?;
    if thai_count(fixed) >= 3 {
        Some(fixed.to_string())
    } else {
        None
    }
}

fn has_utf16_nulls(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }
    let zeros = bytes.iter().step_by(2).filter(|b| **b == 0).count()
        + bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    zeros * 4 > bytes.len()
}

fn decode_sample(enc: &'static Encoding, bytes: &[u8]) -> (String, bool) {
    let (text, _, had_errors) = enc.decode(bytes);
    (text.into_owned(), had_errors)
}

fn plan_from_bytes(bytes: &[u8]) -> (Plan, EncodingReport) {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _) = decode_sample(UTF_16LE, bytes);
        let thai = thai_count(&text);
        return (
            Plan::Decode(UTF_16LE),
            EncodingReport {
                encoding: "utf-16le".into(),
                already_ok: false,
                thai_chars: thai,
                preview: clip_preview(&text),
                confidence: "high".into(),
            },
        );
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _) = decode_sample(UTF_16BE, bytes);
        let thai = thai_count(&text);
        return (
            Plan::Decode(UTF_16BE),
            EncodingReport {
                encoding: "utf-16be".into(),
                already_ok: false,
                thai_chars: thai,
                preview: clip_preview(&text),
                confidence: "high".into(),
            },
        );
    }

    let skip_bom = bytes.starts_with(UTF8_BOM);
    let body = if skip_bom { &bytes[3..] } else { bytes };

    if let Ok(s) = std::str::from_utf8(body) {
        if let Some(fixed) = try_mojibake(s) {
            let thai = thai_count(&fixed);
            return (
                Plan::Mojibake,
                EncodingReport {
                    encoding: "utf-8-mojibake".into(),
                    already_ok: false,
                    thai_chars: thai,
                    preview: clip_preview(&fixed),
                    confidence: "high".into(),
                },
            );
        }
        let thai = thai_count(s);
        return (
            Plan::Utf8AddBom,
            EncodingReport {
                encoding: "utf-8".into(),
                already_ok: skip_bom,
                thai_chars: thai,
                preview: clip_preview(s),
                confidence: "high".into(),
            },
        );
    }

    if has_utf16_nulls(bytes) && bytes.len() % 2 == 0 {
        let (text, err) = decode_sample(UTF_16LE, bytes);
        if !err && thai_count(&text) > 0 {
            return (
                Plan::Decode(UTF_16LE),
                EncodingReport {
                    encoding: "utf-16le".into(),
                    already_ok: false,
                    thai_chars: thai_count(&text),
                    preview: clip_preview(&text),
                    confidence: "medium".into(),
                },
            );
        }
    }

    let (text, had_errors) = decode_sample(WINDOWS_874, body);
    let thai = thai_count(&text);
    let confidence = if thai >= 3 && !had_errors {
        "high"
    } else if thai >= 1 {
        "medium"
    } else {
        "low"
    };
    (
        Plan::Decode(WINDOWS_874),
        EncodingReport {
            encoding: "windows-874".into(),
            already_ok: false,
            thai_chars: thai,
            preview: clip_preview(&text),
            confidence: confidence.into(),
        },
    )
}

fn read_sample(path: &Path) -> EncodeResult<Vec<u8>> {
    let mut file = File::open(path).map_err(|e| format!("failed to open file: {e}"))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; SAMPLE_BYTES];
    let n = file
        .read(&mut tmp)
        .map_err(|e| format!("failed to read file: {e}"))?;
    buf.extend_from_slice(&tmp[..n]);
    Ok(buf)
}

pub fn inspect(path: &Path) -> EncodeResult<EncodingReport> {
    let sample = read_sample(path)?;
    if sample.is_empty() {
        return Err("file is empty".into());
    }
    Ok(plan_from_bytes(&sample).1)
}

pub fn convert(input: &Path, output: &Path) -> EncodeResult<EncodingReport> {
    let sample = read_sample(input)?;
    if sample.is_empty() {
        return Err("file is empty".into());
    }
    let (plan, report) = plan_from_bytes(&sample);
    let mut out = File::create(output).map_err(|e| format!("failed to create output: {e}"))?;
    out.write_all(UTF8_BOM)
        .map_err(|e| format!("failed to write BOM: {e}"))?;

    match plan {
        Plan::Utf8AddBom => {
            let mut file = File::open(input).map_err(|e| e.to_string())?;
            let mut head = [0u8; 3];
            let n = file.read(&mut head).map_err(|e| e.to_string())?;
            if n == 3 && head == UTF8_BOM {
                std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
            } else {
                out.write_all(&head[..n]).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
            }
        }
        Plan::Decode(encoding) => {
            let mut file = File::open(input).map_err(|e| e.to_string())?;
            let mut decoder = encoding.new_decoder();
            let mut in_buf = [0u8; 32 * 1024];
            let mut out_buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut in_buf).map_err(|e| e.to_string())?;
                let last = n == 0;
                let mut offset = 0usize;
                loop {
                    let (result, read, written, _) =
                        decoder.decode_to_utf8(&in_buf[offset..n], &mut out_buf, last);
                    out.write_all(&out_buf[..written])
                        .map_err(|e| e.to_string())?;
                    offset += read;
                    match result {
                        encoding_rs::CoderResult::InputEmpty => break,
                        encoding_rs::CoderResult::OutputFull => continue,
                    }
                }
                if last {
                    break;
                }
            }
        }
        Plan::Mojibake => {
            let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
            let start = if bytes.starts_with(UTF8_BOM) { 3 } else { 0 };
            let s = std::str::from_utf8(&bytes[start..]).map_err(|e| e.to_string())?;
            let fixed = try_mojibake(s).ok_or_else(|| "could not repair file".to_string())?;
            out.write_all(fixed.as_bytes()).map_err(|e| e.to_string())?;
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_bytes(path: &Path, bytes: &[u8]) {
        File::create(path).unwrap().write_all(bytes).unwrap();
    }

    #[test]
    fn utf8_thai_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.csv");
        let output = dir.path().join("b.csv");
        write_bytes(&input, "ชื่อ,จังหวัด\nสมชาย,กรุงเทพมหานคร\n".as_bytes());
        let report = convert(&input, &output).unwrap();
        assert_eq!(report.encoding, "utf-8");
        assert!(report.thai_chars > 0);
        let out = std::fs::read(&output).unwrap();
        assert!(out.starts_with(UTF8_BOM));
        assert!(std::str::from_utf8(&out[3..]).unwrap().contains("สมชาย"));
    }

    #[test]
    fn windows_874_thai_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.csv");
        let output = dir.path().join("b.csv");
        let (bytes, _, _) = WINDOWS_874.encode("สวัสดี,กรุงเทพ");
        write_bytes(&input, &bytes);
        let report = convert(&input, &output).unwrap();
        assert_eq!(report.encoding, "windows-874");
        let out = std::fs::read(&output).unwrap();
        let text = std::str::from_utf8(&out[3..]).unwrap();
        assert!(text.contains("สวัสดี"));
        assert!(text.contains("กรุงเทพ"));
    }

    #[test]
    fn utf8_mojibake_repairs() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.csv");
        let output = dir.path().join("b.csv");
        let mojibake: String = "สวัสดี".as_bytes().iter().map(|&b| char::from(b)).collect();
        write_bytes(&input, format!("name,{mojibake}\n").as_bytes());
        let report = convert(&input, &output).unwrap();
        assert_eq!(report.encoding, "utf-8-mojibake");
        let out = std::fs::read(&output).unwrap();
        let text = std::str::from_utf8(&out[3..]).unwrap();
        assert!(text.contains("สวัสดี"), "{text}");
    }

    #[test]
    fn utf16le_bom_excel_unicode_text() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.txt");
        let output = dir.path().join("b.txt");
        let mut bytes = vec![0xFF, 0xFE];
        for u in "คอลัมน์\tค่า\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        write_bytes(&input, &bytes);
        let report = convert(&input, &output).unwrap();
        assert_eq!(report.encoding, "utf-16le");
        let out = std::fs::read(&output).unwrap();
        let text = std::str::from_utf8(&out[3..]).unwrap();
        assert!(text.contains("คอลัมน์"));
    }
}
