use serde_json::{json, Value};

/// Build an OpenAI-style embeddings request body.
pub fn embed_request_body(model: &str, texts: &[String]) -> Value {
    json!({ "model": model, "input": texts })
}

/// Extract the `index`-th embedding vector from an embeddings response.
pub fn parse_embedding_response(resp: &Value, index: usize) -> Result<Vec<f32>, String> {
    let data = resp
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "embeddings response missing data array".to_string())?;
    let entry = data
        .get(index)
        .ok_or_else(|| format!("embeddings response has no entry at index {index}"))?;
    let embedding = entry
        .get("embedding")
        .and_then(|e| e.as_array())
        .ok_or_else(|| format!("embeddings response entry {index} missing embedding array"))?;
    embedding
        .iter()
        .map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| format!("non-numeric value in embedding at index {index}")))
        .collect()
}

/// Embed each of `texts` in one request. Reads `ZORP_BASE_URL`,
/// `ZORP_API_KEY`, and `ZORP_EMBEDDING_MODEL` from the environment; a
/// missing `ZORP_EMBEDDING_MODEL` is an error, unlike `ZORP_BASE_URL`
/// and `ZORP_MODEL` (chat completions), which have defaults, embeddings
/// have no sensible default model to fall back to.
pub fn embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let base = std::env::var("ZORP_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let key = std::env::var("ZORP_API_KEY").ok();
    let model = std::env::var("ZORP_EMBEDDING_MODEL")
        .map_err(|_| "ZORP_EMBEDDING_MODEL is not set".to_string())?;

    let url = zorp::join_url(&base, "embeddings");
    let body = embed_request_body(&model, texts);
    let auth = key.map(|k| format!("Bearer {k}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    let resp = zorp::zorp_raw(&url, &headers, body).map_err(|e| e.to_string())?;

    (0..texts.len())
        .map(|i| parse_embedding_response(&resp, i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_request_body_shape() {
        let body = embed_request_body("text-embedding-3-small", &["a".to_string(), "b".to_string()]);
        assert_eq!(body["model"], "text-embedding-3-small");
        assert_eq!(body["input"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn parse_embedding_response_ok() {
        let resp = json!({ "data": [{ "embedding": [0.1, 0.2, 0.3] }] });
        let v = parse_embedding_response(&resp, 0).unwrap();
        assert_eq!(v.len(), 3);
        assert!((v[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn parse_embedding_response_missing_data_errs() {
        let resp = json!({ "error": "bad request" });
        assert!(parse_embedding_response(&resp, 0).is_err());
    }

    #[test]
    fn parse_embedding_response_index_out_of_range_errs() {
        let resp = json!({ "data": [{ "embedding": [0.1] }] });
        assert!(parse_embedding_response(&resp, 1).is_err());
    }

    #[test]
    fn parse_embedding_response_non_numeric_value_errs() {
        let resp = json!({ "data": [{ "embedding": ["not", "numbers"] }] });
        assert!(parse_embedding_response(&resp, 0).is_err());
    }
}
