use async_trait::async_trait;
use aws_credential_types::Credentials;
use bytes::{Buf, Bytes, BytesMut};
use futures_util::StreamExt;
use http::HeaderMap;
use reqwest::Client;
use serde_json::json;
use tracing::debug;

use crate::auth::aws::sign_request;
use crate::error::ProviderError;
use crate::provider::model_map::translate_model_id;
use crate::provider::{Provider, SseByteStream};
use crate::types::ProviderKind;

const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

pub struct BedrockProvider {
    client: Client,
    credentials: Credentials,
    region: String,
    /// Pre-computed host header value: "bedrock-runtime.{region}.amazonaws.com"
    host_header: String,
    /// Pre-computed base URL: "https://bedrock-runtime.{region}.amazonaws.com"
    base_url: String,
}

impl BedrockProvider {
    pub fn new(credentials: Credentials, region: String) -> Self {
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(32)
            .tcp_nodelay(true)
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        let host_header = format!("bedrock-runtime.{}.amazonaws.com", region);
        let base_url = format!("https://{}", host_header);

        Self {
            client,
            credentials,
            region,
            host_header,
            base_url,
        }
    }

    fn transform_body(&self, mut body: serde_json::Value) -> (String, serde_json::Value) {
        let obj = body.as_object_mut().expect("body must be an object");

        // Extract and translate model ID
        let model = obj
            .remove("model")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());

        let bedrock_model = translate_model_id(ProviderKind::Bedrock, &model);

        // Remove stream field (endpoint determines streaming)
        obj.remove("stream");

        // Add anthropic_version
        obj.insert(
            "anthropic_version".to_string(),
            json!(BEDROCK_ANTHROPIC_VERSION),
        );

        (bedrock_model, body)
    }

    async fn send_signed_request(
        &self,
        url: &str,
        body_bytes: Vec<u8>,
    ) -> Result<reqwest::Response, ProviderError> {
        // Use pre-computed static header values; no per-request String allocation
        let headers = vec![
            ("content-type", "application/json"),
            ("host", &self.host_header),
        ];

        let signed_headers =
            sign_request("POST", url, &headers, &body_bytes, &self.credentials, &self.region)
                .map_err(|e| ProviderError::Auth {
                    provider: ProviderKind::Bedrock,
                    message: format!("SigV4 signing failed: {}", e),
                })?;

        let mut req = self.client.post(url);
        for (name, value) in &signed_headers {
            req = req.header(name.as_str(), value.as_str());
        }

        // Move body_bytes directly into the request - no extra copy
        req.body(body_bytes)
            .send()
            .await
            .map_err(|e| ProviderError::Connection {
                provider: ProviderKind::Bedrock,
                source: e,
            })
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Bedrock
    }

    async fn messages_stream(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<SseByteStream, ProviderError> {
        debug!("Bedrock messages_stream request");

        let (model_id, transformed_body) = self.transform_body(body);
        let url = format!(
            "{}/model/{}/invoke-with-response-stream",
            self.base_url,
            urlencoding::encode(&model_id)
        );

        let body_bytes = serde_json::to_vec(&transformed_body)
            .map_err(|e| ProviderError::Transform(e.to_string()))?;

        let response = self.send_signed_request(&url, body_bytes).await?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Bedrock,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        // Decode AWS Event Stream binary format and re-emit as SSE
        let stream = decode_event_stream(response);
        Ok(Box::pin(stream))
    }

    async fn messages(
        &self,
        body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Bedrock messages request (non-streaming)");

        let (model_id, transformed_body) = self.transform_body(body);
        let url = format!(
            "{}/model/{}/invoke",
            self.base_url,
            urlencoding::encode(&model_id)
        );

        let body_bytes = serde_json::to_vec(&transformed_body)
            .map_err(|e| ProviderError::Transform(e.to_string()))?;

        let response = self.send_signed_request(&url, body_bytes).await?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                provider: ProviderKind::Bedrock,
                status,
                message: format!("HTTP {}", status),
                body: Some(body),
            });
        }

        response.json().await.map_err(|e| ProviderError::Connection {
            provider: ProviderKind::Bedrock,
            source: e,
        })
    }

    async fn count_tokens(
        &self,
        _body: serde_json::Value,
        _headers: &HeaderMap,
    ) -> Result<serde_json::Value, ProviderError> {
        debug!("Bedrock count_tokens request");

        Err(ProviderError::Upstream {
            provider: ProviderKind::Bedrock,
            status: 501,
            message: "count_tokens is not supported by Bedrock".to_string(),
            body: None,
        })
    }

    async fn list_models(&self) -> Result<serde_json::Value, ProviderError> {
        debug!("Bedrock list_models request");

        Ok(json!({
            "data": [
                {"id": "claude-opus-4-6", "display_name": "Claude Opus 4.6", "provider": "bedrock"},
                {"id": "claude-sonnet-4-5-20250929", "display_name": "Claude Sonnet 4.5", "provider": "bedrock"},
                {"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "provider": "bedrock"},
                {"id": "claude-3-5-sonnet-20241022", "display_name": "Claude 3.5 Sonnet", "provider": "bedrock"},
                {"id": "claude-3-5-haiku-20241022", "display_name": "Claude 3.5 Haiku", "provider": "bedrock"},
            ],
            "has_more": false,
        }))
    }
}

/// Decode AWS Event Stream binary format from a Bedrock response
/// and re-emit as SSE text format.
///
/// Optimized: uses a single reusable write buffer to avoid per-event allocations.
/// The inner JSON payload is written directly to the SSE buffer without intermediate
/// String allocation. Base64 decoding reuses a persistent buffer.
fn decode_event_stream(
    response: reqwest::Response,
) -> impl futures_util::Stream<Item = Result<Bytes, ProviderError>> {
    async_stream::stream! {
        let mut byte_stream = response.bytes_stream();
        let mut buffer = BytesMut::with_capacity(16 * 1024); // 16 KB initial capacity
        let mut decode_buf = Vec::with_capacity(4096); // Reusable base64 decode buffer
        let mut sse_buf = Vec::with_capacity(4096); // Reusable SSE output buffer

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    yield Err(ProviderError::StreamDecode(format!(
                        "Stream read error: {}", e
                    )));
                    return;
                }
            };

            buffer.extend_from_slice(&chunk);

            // Process all complete frames in this buffer
            loop {
                if buffer.len() < 12 {
                    break;
                }

                let total_len = u32::from_be_bytes([
                    buffer[0], buffer[1], buffer[2], buffer[3],
                ]) as usize;

                if buffer.len() < total_len {
                    break;
                }

                let headers_len = u32::from_be_bytes([
                    buffer[4], buffer[5], buffer[6], buffer[7],
                ]) as usize;

                let payload_start = 12 + headers_len;
                let payload_end = total_len - 4;

                if payload_start <= payload_end && payload_end <= buffer.len() {
                    let payload = &buffer[payload_start..payload_end];

                    // Minimal JSON parsing: only extract the "bytes" field value.
                    // We avoid parsing into serde_json::Value by scanning for the key.
                    if let Some(b64_bytes) = extract_bytes_field(payload) {
                        // Decode base64 into reusable buffer
                        decode_buf.clear();
                        if let Some(n) = base64_decode_into(b64_bytes, &mut decode_buf) {
                            // The decoded data IS the event JSON. Extract only the
                            // "type" field for the SSE event name, then emit the
                            // raw decoded bytes as the data payload (no re-serialization).
                            let event_data = &decode_buf[..n];
                            let event_type = extract_type_field(event_data)
                                .unwrap_or(b"content_block_delta");

                            // Build SSE frame directly in reusable buffer
                            sse_buf.clear();
                            sse_buf.extend_from_slice(b"event: ");
                            sse_buf.extend_from_slice(event_type);
                            sse_buf.extend_from_slice(b"\ndata: ");
                            sse_buf.extend_from_slice(event_data);
                            sse_buf.extend_from_slice(b"\n\n");

                            yield Ok(Bytes::copy_from_slice(&sse_buf));
                        }
                    }
                }

                buffer.advance(total_len);
            }
        }
    }
}

/// Extract the raw bytes of the "bytes" field value from a JSON payload
/// without full parsing. Returns the base64 string bytes (without quotes).
///
/// Scans for `"bytes":"<value>"` or `"bytes": "<value>"` in the payload.
fn extract_bytes_field(json: &[u8]) -> Option<&[u8]> {
    // Look for "bytes":" or "bytes": "
    let needle = b"\"bytes\"";
    let pos = find_subsequence(json, needle)?;
    let rest = &json[pos + needle.len()..];

    // Skip optional whitespace and colon
    let rest = skip_ws_colon(rest)?;

    // Expect opening quote
    if rest.first() != Some(&b'"') {
        return None;
    }
    let rest = &rest[1..];

    // Find closing quote (base64 doesn't contain backslash-escaped chars)
    let end = memchr::memchr(b'"', rest)?;
    Some(&rest[..end])
}

/// Extract the "type" field value as raw bytes from event JSON.
fn extract_type_field(json: &[u8]) -> Option<&[u8]> {
    let needle = b"\"type\"";
    let pos = find_subsequence(json, needle)?;
    let rest = &json[pos + needle.len()..];
    let rest = skip_ws_colon(rest)?;
    if rest.first() != Some(&b'"') {
        return None;
    }
    let rest = &rest[1..];
    let end = memchr::memchr(b'"', rest)?;
    Some(&rest[..end])
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn skip_ws_colon(data: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    // Skip whitespace
    while i < data.len() && (data[i] == b' ' || data[i] == b'\t' || data[i] == b'\n' || data[i] == b'\r') {
        i += 1;
    }
    // Expect colon
    if i >= data.len() || data[i] != b':' {
        return None;
    }
    i += 1;
    // Skip whitespace after colon
    while i < data.len() && (data[i] == b' ' || data[i] == b'\t' || data[i] == b'\n' || data[i] == b'\r') {
        i += 1;
    }
    Some(&data[i..])
}

/// Decode base64 into an existing buffer, returning the number of decoded bytes.
fn base64_decode_into(input: &[u8], output: &mut Vec<u8>) -> Option<usize> {
    use base64::Engine;
    let needed = input.len(); // Over-estimate is fine
    output.resize(needed, 0);
    base64::engine::general_purpose::STANDARD
        .decode_slice(input, output)
        .ok()
}
