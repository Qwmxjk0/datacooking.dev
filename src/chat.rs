use crate::error::AppError;
use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MAX_MESSAGES: usize = 24;
const MAX_CHARS: usize = 4_000;
const MAX_TOKENS: u32 = 512;

#[derive(Debug, Deserialize)]
pub struct ChatIn {
    pub messages: Vec<ChatMsg>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub think: Option<bool>,
    pub system: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
}

pub async fn proxy_chat(
    llm_url: &str,
    http: &reqwest::Client,
    body: ChatIn,
) -> Result<Response, AppError> {
    if llm_url.trim().is_empty() {
        return Err(AppError::unavailable("โมเดลยังไม่พร้อมบนเซิร์ฟเวอร์"));
    }

    let mut messages = Vec::new();
    if let Some(system) = body
        .system
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        messages.push(json!({
            "role": "system",
            "content": clip(system, MAX_CHARS),
        }));
    }
    for msg in body
        .messages
        .iter()
        .rev()
        .take(MAX_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let role = match msg.role.as_str() {
            "user" | "assistant" | "system" => msg.role.as_str(),
            _ => continue,
        };
        let content = clip(&msg.content, MAX_CHARS);
        if content.is_empty() {
            continue;
        }
        messages.push(json!({ "role": role, "content": content }));
    }
    if messages.is_empty() {
        return Err(AppError::bad_request("พิมพ์ข้อความก่อน"));
    }

    let think = body.think.unwrap_or(false);
    let temperature = body
        .temperature
        .unwrap_or(if think { 0.9 } else { 0.7 })
        .clamp(0.1, 1.5);
    let top_p = body.top_p.unwrap_or(0.95).clamp(0.1, 1.0);
    let max_tokens = body.max_tokens.unwrap_or(256).clamp(16, MAX_TOKENS);

    let mut payload = json!({
        "model": "MiniCPM5-1B",
        "stream": true,
        "messages": messages,
        "temperature": temperature,
        "top_p": top_p,
        "min_p": 0.0,
        "max_tokens": max_tokens,
    });
    if think {
        payload["chat_template_kwargs"] = json!({ "enable_thinking": true });
    }

    let url = format!("{}/v1/chat/completions", llm_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|_| AppError::unavailable("โมเดลบนเซิร์ฟเวอร์ยังไม่ตอบ"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::warn!(%status, %text, "llm upstream error");
        return Err(AppError::unavailable("โมเดลบนเซิร์ฟเวอร์ยังไม่พร้อม"));
    }

    let stream = resp.bytes_stream();
    Ok((
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

pub async fn llm_ready(llm_url: &str, http: &reqwest::Client) -> bool {
    if llm_url.trim().is_empty() {
        return false;
    }
    let url = format!("{}/health", llm_url.trim_end_matches('/'));
    http.get(url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

fn clip(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(max)
        .collect()
}
