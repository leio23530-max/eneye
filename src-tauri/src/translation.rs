use std::{collections::HashMap, sync::Mutex};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::settings::AppSettings;

const KEYRING_SERVICE: &str = "app.eneye.desktop.ai";
const KEYRING_USER: &str = "default";
const MAX_SELECTION_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRequest {
    pub request_id: String,
    pub text: String,
    pub source_language: String,
    pub target_language: String,
    #[serde(default = "default_source")]
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationOutcome {
    pub text: String,
    pub unsupported_reasoning_model: Option<String>,
}

fn default_source() -> String {
    "uia".into()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum TranslationEvent {
    Started { request_id: String },
    Delta { request_id: String, text: String },
    Completed { request_id: String, text: String },
    Failed { request_id: String, error: String },
    Cancelled { request_id: String },
}

#[derive(Default)]
pub struct TranslationTasks {
    tasks: Mutex<HashMap<String, CancellationToken>>,
}

impl TranslationTasks {
    pub fn insert(&self, request_id: String, token: CancellationToken) -> Result<()> {
        let mut tasks = self
            .tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("translation task lock is poisoned"))?;
        if let Some(previous) = tasks.insert(request_id, token) {
            previous.cancel();
        }
        Ok(())
    }

    pub fn remove(&self, request_id: &str) -> Result<Option<CancellationToken>> {
        Ok(self
            .tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("translation task lock is poisoned"))?
            .remove(request_id))
    }

    pub fn cancel(&self, request_id: &str) -> Result<bool> {
        let token = self
            .tasks
            .lock()
            .map_err(|_| anyhow::anyhow!("translation task lock is poisoned"))?
            .remove(request_id);
        if let Some(token) = token {
            token.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

pub struct AiKeyStore;

impl AiKeyStore {
    pub fn set(api_key: &str) -> Result<()> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        let key = api_key.trim();
        if key.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(error.into()),
            }
        } else {
            entry.set_password(key)?;
            Ok(())
        }
    }

    pub fn get() -> Result<String> {
        let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
        entry
            .get_password()
            .context("API Key 尚未配置；请先在“AI 翻译”中编辑 API 连接并保存密钥")
    }

    pub fn exists() -> bool {
        Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .and_then(|entry| entry.get_password())
            .is_ok()
    }
}

#[derive(Debug, Default)]
pub struct StreamParser {
    buffer: String,
    finished: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedDelta {
    pub text: Option<String>,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProtocol {
    OpenAi,
    Anthropic,
}

impl AiProtocol {
    pub fn for_settings(settings: &AppSettings) -> Self {
        match settings.ai_protocol.as_str() {
            "anthropic" => Self::Anthropic,
            "openai" => Self::OpenAi,
            _ if settings
                .ai_base_url
                .to_ascii_lowercase()
                .contains("anthropic") =>
            {
                Self::Anthropic
            }
            _ => Self::OpenAi,
        }
    }
}

impl StreamParser {
    pub fn push(&mut self, chunk: &str) -> Result<Vec<ParsedDelta>> {
        self.buffer.push_str(chunk);
        let mut parsed = Vec::new();
        while let Some(newline) = self.buffer.find('\n') {
            let line = self.buffer.drain(..=newline).collect::<String>();
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                self.finished = data == "[DONE]";
                parsed.push(ParsedDelta {
                    text: None,
                    finished: self.finished,
                });
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(data).context("AI returned malformed stream JSON")?;
            if let Some(error) = value.get("error") {
                anyhow::bail!("AI service error: {error}");
            }
            let text = value
                .pointer("/choices/0/delta/content")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    value
                        .pointer("/delta/text")
                        .and_then(serde_json::Value::as_str)
                })
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned);
            if value["type"].as_str() == Some("message_stop") {
                self.finished = true;
                parsed.push(ParsedDelta {
                    text: None,
                    finished: true,
                });
            }
            if text.is_some() {
                parsed.push(ParsedDelta {
                    text,
                    finished: false,
                });
            }
        }
        Ok(parsed)
    }

    pub fn finished(&self) -> bool {
        self.finished
    }
}

pub async fn stream_translation<F>(
    settings: &AppSettings,
    api_key: &str,
    request: &TranslationRequest,
    cancel: CancellationToken,
    mut emit: F,
) -> Result<TranslationOutcome>
where
    F: FnMut(TranslationEvent),
{
    validate_request(request)?;
    emit(TranslationEvent::Started {
        request_id: request.request_id.clone(),
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let protocol = AiProtocol::for_settings(settings);
    let endpoint = translation_endpoint(&settings.ai_base_url, protocol)?;
    let source_language = if request.source_language.trim().is_empty() {
        "auto"
    } else {
        request.source_language.trim()
    };
    let target_language = if request.target_language.trim().is_empty() {
        "简体中文"
    } else {
        request.target_language.trim()
    };
    let instruction = format!(
        "You are a translation engine. Translate the user's text from {source_language} to {target_language}. Return only the translation, preserve paragraph breaks, and do not add explanations."
    );
    let model = settings.translation_model();
    let mut body = match protocol {
        AiProtocol::OpenAi => json!({
            "model": model,
            "stream": true,
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": instruction },
                { "role": "user", "content": request.text }
            ]
        }),
        AiProtocol::Anthropic => json!({
            "model": model,
            "stream": true,
            "max_tokens": 2048,
            "temperature": 0.2,
            "system": instruction,
            "messages": [
                { "role": "user", "content": request.text }
            ]
        }),
    };
    if protocol == AiProtocol::OpenAi && settings.supports_reasoning_effort(model) {
        body.as_object_mut().map(|object| {
            object.insert(
                "reasoning_effort".into(),
                json!(settings.translation_effort()),
            )
        });
    }
    let mut unsupported_reasoning_model = None;

    let mut response = tokio::select! {
        _ = cancel.cancelled() => {
            emit(TranslationEvent::Cancelled { request_id: request.request_id.clone() });
            anyhow::bail!("translation cancelled");
        }
        response = authorized_request(client.post(&endpoint), protocol, api_key)
            .json(&body)
            .send() => response.context("无法连接 AI 服务")?
    };

    if response.status() == reqwest::StatusCode::BAD_REQUEST {
        let error_body = response.text().await.unwrap_or_default();
        let safe_error_body = redact_secret(&error_body, api_key);
        let has_reasoning_effort = body
            .as_object()
            .is_some_and(|object| object.contains_key("reasoning_effort"));
        if has_reasoning_effort && error_body.to_ascii_lowercase().contains("reasoning_effort") {
            body.as_object_mut()
                .map(|object| object.remove("reasoning_effort"));
            unsupported_reasoning_model = Some(model.to_owned());
            response = tokio::select! {
                _ = cancel.cancelled() => {
                    emit(TranslationEvent::Cancelled { request_id: request.request_id.clone() });
                    anyhow::bail!("translation cancelled");
                }
                response = authorized_request(client.post(&endpoint), protocol, api_key)
                    .json(&body)
                    .send() => response.context("无法连接 AI 服务")?
            };
        } else {
            anyhow::bail!(
                "AI 服务返回 400：{}",
                response_error_hint(400, &safe_error_body)
            );
        }
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let safe_body = redact_secret(&body, api_key)
            .chars()
            .take(500)
            .collect::<String>();
        anyhow::bail!(
            "AI 服务返回 {}：{}",
            status.as_u16(),
            response_error_hint(status.as_u16(), &safe_body)
        );
    }

    let mut stream = response.bytes_stream();
    let mut parser = StreamParser::default();
    let mut output = String::new();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => {
                emit(TranslationEvent::Cancelled { request_id: request.request_id.clone() });
                anyhow::bail!("translation cancelled");
            }
            next = stream.next() => next
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.context("AI 流读取中断")?;
        let text = String::from_utf8_lossy(&chunk);
        for parsed in parser.push(&text)? {
            if let Some(delta) = parsed.text {
                output.push_str(&delta);
                emit(TranslationEvent::Delta {
                    request_id: request.request_id.clone(),
                    text: delta,
                });
            }
        }
        if parser.finished() {
            break;
        }
    }

    if !parser.finished() {
        anyhow::bail!("AI 流意外中断，请重试");
    }
    if output.trim().is_empty() {
        anyhow::bail!("AI 服务没有返回译文");
    }
    emit(TranslationEvent::Completed {
        request_id: request.request_id.clone(),
        text: output.clone(),
    });
    Ok(TranslationOutcome {
        text: output,
        unsupported_reasoning_model,
    })
}

fn validate_request(request: &TranslationRequest) -> Result<()> {
    let text = request.text.trim();
    if text.is_empty() {
        anyhow::bail!("选区为空");
    }
    if text.chars().count() > MAX_SELECTION_CHARS {
        anyhow::bail!("选区超过 {MAX_SELECTION_CHARS} 个字符");
    }
    Ok(())
}

pub fn translation_endpoint(base_url: &str, protocol: AiProtocol) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        anyhow::bail!("AI Base URL 尚未配置");
    }
    let parsed = url::Url::parse(base).context("AI Base URL 无效")?;
    let is_loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && is_loopback) {
        anyhow::bail!("AI Base URL 必须使用 HTTPS，或指向本机回环地址");
    }
    let endpoint = match protocol {
        AiProtocol::OpenAi if base.ends_with("/chat/completions") => base.to_owned(),
        AiProtocol::OpenAi => format!("{base}/chat/completions"),
        AiProtocol::Anthropic if base.ends_with("/messages") => base.to_owned(),
        AiProtocol::Anthropic if base.ends_with("/v1") => format!("{base}/messages"),
        AiProtocol::Anthropic => format!("{base}/v1/messages"),
    };
    Ok(endpoint)
}

pub async fn list_models(settings: &AppSettings, api_key: &str) -> Result<Vec<String>> {
    let protocol = AiProtocol::for_settings(settings);
    let endpoint = models_endpoint(&settings.ai_base_url, protocol)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let response = authorized_request(client.get(endpoint), protocol, api_key)
        .send()
        .await
        .context("无法连接 AI 服务")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let body = redact_secret(&body, api_key);
        anyhow::bail!(
            "读取模型失败：{} {}",
            status.as_u16(),
            response_error_hint(status.as_u16(), &body)
        );
    }
    let value: serde_json::Value = response.json().await?;
    let mut models = value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    models.sort_by_key(|model| model.to_ascii_lowercase());
    models.dedup();
    Ok(models)
}

fn models_endpoint(base_url: &str, protocol: AiProtocol) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    let base = base
        .strip_suffix("/chat/completions")
        .or_else(|| base.strip_suffix("/messages"))
        .unwrap_or(base);
    let parsed = url::Url::parse(base).context("AI Base URL 无效")?;
    let is_loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && is_loopback) {
        anyhow::bail!("AI Base URL 必须使用 HTTPS，或指向本机回环地址");
    }
    Ok(match (protocol, base.ends_with("/v1")) {
        (AiProtocol::Anthropic, false) => format!("{base}/v1/models"),
        _ => format!("{base}/models"),
    })
}

fn authorized_request(
    request: reqwest::RequestBuilder,
    protocol: AiProtocol,
    api_key: &str,
) -> reqwest::RequestBuilder {
    match protocol {
        AiProtocol::OpenAi => request.bearer_auth(api_key),
        AiProtocol::Anthropic => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
    }
}

fn response_error_hint(status: u16, body: &str) -> String {
    match status {
        401 | 403 => "API Key 无效或没有模型权限".into(),
        404 => "接口或模型不存在，请检查 Base URL 与模型名称".into(),
        429 => "请求过于频繁或额度不足，请稍后重试".into(),
        500..=599 => "服务暂时不可用，请稍后重试".into(),
        _ if !body.trim().is_empty() => body.trim().to_owned(),
        _ => "请求失败".into(),
    }
}

fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_owned()
    } else {
        text.replace(secret, "<REDACTED>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_handles_split_sse_chunks_and_done() {
        let mut parser = StreamParser::default();
        assert!(
            parser
                .push("data: {\"choices\":[{\"delta\":{\"content\":\"你")
                .unwrap()
                .is_empty()
        );
        let events = parser.push("好\"}}]}\n\ndata: [DONE]\n\n").unwrap();
        assert_eq!(events[0].text.as_deref(), Some("你好"));
        assert!(events[1].finished);
    }

    #[test]
    fn appends_endpoint_only_when_needed() {
        assert_eq!(
            translation_endpoint("https://api.example.com/v1", AiProtocol::OpenAi).unwrap(),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            translation_endpoint(
                "https://api.example.com/v1/chat/completions",
                AiProtocol::OpenAi
            )
            .unwrap(),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            translation_endpoint("https://api.anthropic.com", AiProtocol::Anthropic).unwrap(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn rejects_plain_http_outside_loopback() {
        assert!(translation_endpoint("http://api.example.com/v1", AiProtocol::OpenAi).is_err());
        assert!(translation_endpoint("http://127.0.0.1:8080/v1", AiProtocol::OpenAi).is_ok());
    }

    #[test]
    fn parser_reads_anthropic_text_deltas() {
        let mut parser = StreamParser::default();
        let events = parser
            .push(
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"译文\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n",
            )
            .unwrap();
        assert_eq!(events[0].text.as_deref(), Some("译文"));
        assert!(events[1].finished);
    }

    #[test]
    fn derives_protocol_and_model_endpoints_like_neoread() {
        let anthropic = AppSettings {
            ai_protocol: "auto".into(),
            ai_base_url: "https://api.anthropic.com".into(),
            ..AppSettings::default()
        };
        assert_eq!(AiProtocol::for_settings(&anthropic), AiProtocol::Anthropic);
        assert_eq!(
            models_endpoint(&anthropic.ai_base_url, AiProtocol::Anthropic).unwrap(),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            models_endpoint("https://api.deepseek.com", AiProtocol::OpenAi).unwrap(),
            "https://api.deepseek.com/models"
        );
    }

    #[test]
    fn serializes_event_fields_for_the_typescript_contract() {
        let event = TranslationEvent::Failed {
            request_id: "request-1".into(),
            error: "no key".into(),
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["type"], "failed");
        assert_eq!(json["requestId"], "request-1");
        assert_eq!(json["error"], "no key");
    }

    #[test]
    fn redacts_api_keys_from_provider_error_text() {
        let message = redact_secret("request rejected for key sk-secret-123", "sk-secret-123");
        assert_eq!(message, "request rejected for key <REDACTED>");
    }

    #[tokio::test]
    async fn streams_from_an_openai_compatible_mock_server() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let settings = AppSettings {
            ai_base_url: format!("http://{address}/v1"),
            ai_model: "mock-model".into(),
            ..AppSettings::default()
        };
        let request = TranslationRequest {
            request_id: "test".into(),
            text: "Hello".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            source: "uia".into(),
        };
        let mut events = Vec::new();
        let result = stream_translation(
            &settings,
            "test-key",
            &request,
            CancellationToken::new(),
            |event| events.push(event),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(result.text, "你好");
        assert_eq!(result.unsupported_reasoning_model, None);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, TranslationEvent::Completed { .. }))
        );
    }

    #[tokio::test]
    async fn retries_without_reasoning_effort_when_endpoint_rejects_it() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0_u8; 16 * 1024];
                let read = socket.read(&mut request).await.unwrap();
                requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
                if attempt == 0 {
                    let body = r#"{"error":{"message":"unsupported parameter reasoning_effort"}}"#;
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                } else {
                    let body = concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"成功\"}}]}\n\n",
                        "data: [DONE]\n\n"
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            }
            requests
        });

        let mut settings = AppSettings {
            ai_base_url: format!("http://{address}/v1"),
            ai_model: "mock-model".into(),
            ..AppSettings::default()
        };
        settings
            .ai_roles
            .translation
            .model
            .clone_from(&settings.ai_model);
        settings.ai_roles.translation.effort = "medium".into();
        let request = TranslationRequest {
            request_id: "retry".into(),
            text: "Hello".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            source: "uia".into(),
        };
        let outcome = stream_translation(
            &settings,
            "test-key",
            &request,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let requests = server.await.unwrap();
        assert_eq!(outcome.text, "成功");
        assert_eq!(
            outcome.unsupported_reasoning_model.as_deref(),
            Some("mock-model")
        );
        assert!(requests[0].contains("reasoning_effort"));
        assert!(!requests[1].contains("reasoning_effort"));
    }

    #[tokio::test]
    async fn omits_reasoning_effort_for_a_remembered_model() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let read = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"成功\"}}]}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            request
        });

        let mut settings = AppSettings {
            ai_base_url: format!("http://{address}/v1"),
            ai_model: "compatible-model".into(),
            ..AppSettings::default()
        };
        assert!(settings.remember_unsupported_reasoning_model("compatible-model"));
        let request = TranslationRequest {
            request_id: "remembered".into(),
            text: "Hello".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            source: "uia".into(),
        };
        let outcome = stream_translation(
            &settings,
            "test-key",
            &request,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let request = server.await.unwrap();
        assert_eq!(outcome.text, "成功");
        assert!(!request.contains("reasoning_effort"));
    }

    #[tokio::test]
    async fn reports_invalid_api_keys_without_echoing_the_key() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            let body = r#"{"error":"invalid key sk-secret-123"}"#;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let settings = AppSettings {
            ai_base_url: format!("http://{address}/v1"),
            ai_model: "mock-model".into(),
            ..AppSettings::default()
        };
        let request = TranslationRequest {
            request_id: "invalid-key".into(),
            text: "Hello".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            source: "uia".into(),
        };
        let error = stream_translation(
            &settings,
            "sk-secret-123",
            &request,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err()
        .to_string();
        server.await.unwrap();
        assert!(error.contains("API Key 无效"));
        assert!(!error.contains("sk-secret-123"));
    }

    #[tokio::test]
    async fn rejects_a_stream_that_ends_without_a_completion_marker() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"半句\"}}]}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let settings = AppSettings {
            ai_base_url: format!("http://{address}/v1"),
            ai_model: "mock-model".into(),
            ..AppSettings::default()
        };
        let request = TranslationRequest {
            request_id: "interrupted".into(),
            text: "Hello".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            source: "uia".into(),
        };
        let error = stream_translation(
            &settings,
            "test-key",
            &request,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err()
        .to_string();
        server.await.unwrap();
        assert!(error.contains("AI 流意外中断"));
    }

    #[tokio::test]
    async fn cancellation_before_send_emits_a_cancelled_event() {
        let settings = AppSettings {
            ai_base_url: "http://127.0.0.1:9/v1".into(),
            ai_model: "mock-model".into(),
            ..AppSettings::default()
        };
        let request = TranslationRequest {
            request_id: "cancelled".into(),
            text: "Hello".into(),
            source_language: "auto".into(),
            target_language: "简体中文".into(),
            source: "uia".into(),
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut events = Vec::new();
        let error = stream_translation(&settings, "test-key", &request, cancel, |event| {
            events.push(event)
        })
        .await
        .unwrap_err()
        .to_string();

        assert_eq!(error, "translation cancelled");
        assert!(events.iter().any(|event| matches!(
            event,
            TranslationEvent::Cancelled { request_id } if request_id == "cancelled"
        )));
    }

    #[tokio::test]
    async fn reports_model_listing_http_errors() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            let body = r#"{"error":"unauthorized"}"#;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let settings = AppSettings {
            ai_base_url: format!("http://{address}/v1"),
            ..AppSettings::default()
        };
        let error = list_models(&settings, "test-key")
            .await
            .unwrap_err()
            .to_string();
        server.await.unwrap();
        assert!(error.contains("读取模型失败：401"));
    }
}
