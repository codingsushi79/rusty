//! AI client — OpenAI-compatible `/chat/completions`.
//!
//! Works with a **local** server (Ollama's OpenAI-compatible API at
//! `http://localhost:11434/v1`, or llama.cpp) with no token, **and** with any
//! cloud provider by bringing your own token (OpenAI, OpenRouter, Together, …).
//! Off by default; nothing runs unless AI is enabled in Settings.

use std::io::BufRead;

use serde_json::{json, Value};

fn base(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// Is the endpoint reachable?
pub fn available(endpoint: &str, api_key: &str) -> bool {
    let mut req = ureq::get(&format!("{}/models", base(endpoint)));
    if !api_key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {api_key}"));
    }
    req.call().is_ok()
}

/// Stream a reply, invoking `on_chunk` for each token/piece as it arrives.
/// Uses the OpenAI-compatible SSE (`data: {…}`) stream.
pub fn generate_stream(
    endpoint: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
    mut on_chunk: impl FnMut(&str),
) -> anyhow::Result<()> {
    let url = format!("{}/chat/completions", base(endpoint));
    let mut req = ureq::post(&url).set("Content-Type", "application/json");
    if !api_key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {api_key}"));
    }
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": true,
    });
    let resp = req.send_string(&serde_json::to_string(&body)?)?;
    let reader = std::io::BufReader::new(resp.into_reader());
    for line in reader.lines() {
        let line = line?;
        let payload = line.strip_prefix("data:").unwrap_or(&line).trim();
        if payload.is_empty() {
            continue;
        }
        if payload == "[DONE]" {
            break;
        }
        if let Ok(v) = serde_json::from_str::<Value>(payload) {
            // OpenAI streams deltas; some servers stream full messages.
            let piece = v["choices"][0]["delta"]["content"]
                .as_str()
                .or_else(|| v["choices"][0]["message"]["content"].as_str());
            if let Some(p) = piece {
                if !p.is_empty() {
                    on_chunk(p);
                }
            }
            if let Some(err) = v["error"]["message"].as_str() {
                on_chunk(&format!("\n[error] {err}"));
            }
        }
    }
    Ok(())
}
