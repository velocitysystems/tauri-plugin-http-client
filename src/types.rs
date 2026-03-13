use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Encoding used to transport the request body over IPC.
///
/// The TypeScript guest constrains this to `'utf8' | 'base64'`; the Rust
/// enum mirrors that constraint so serde rejects unknown values at
/// deserialization time.
#[derive(Debug, PartialEq, Deserialize)]
pub enum BodyEncoding {
   #[serde(rename = "utf8")]
   Utf8,
   #[serde(rename = "base64")]
   Base64,
}

/// Request payload sent from the TypeScript guest to the Rust backend via IPC.
///
/// All URL parsing and validation happens exclusively in Rust to avoid
/// JS/Rust URL parsing differentials.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchRequest {
   pub url: String,
   pub method: Option<String>,
   pub headers: Option<HashMap<String, String>>,
   pub body: Option<String>,
   pub body_encoding: Option<BodyEncoding>,
   pub timeout_ms: Option<u64>,
   pub request_id: Option<String>,
   /// Per-request retry override. `None` uses plugin config default.
   /// `Some(0)` disables retry for this request. Capped at the plugin-level
   /// `RetryConfig::max_retries` — the frontend cannot exceed the configured ceiling.
   pub max_retries: Option<u32>,
}

/// HTTP response metadata without the body, serialized as JSON in the binary
/// framing protocol (`[4-byte BE length][metadata JSON][body bytes]`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FetchResponseMetadata {
   pub(crate) status: u16,
   pub(crate) status_text: String,
   pub(crate) headers: HashMap<String, Vec<String>>,
   pub(crate) url: String,
   pub(crate) redirected: bool,
   /// Number of retry attempts that occurred before this response (0 = no retries).
   pub(crate) retry_count: u32,
}

/// Internal result from the HTTP execution pipeline, carrying raw body bytes
/// and response metadata. Converted to a binary-framed IPC response at the
/// command layer.
#[derive(Debug)]
pub(crate) struct ExecuteResult {
   pub metadata: FetchResponseMetadata,
   pub body: Vec<u8>,
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_fetch_request_deserializes_camel_case() {
      let json = serde_json::json!({
         "url": "https://example.com",
         "method": "POST",
         "headers": {"content-type": "application/json"},
         "body": "hello",
         "bodyEncoding": "utf8",
         "timeoutMs": 5000,
         "requestId": "req-1"
      });

      let req: FetchRequest = serde_json::from_value(json).unwrap();

      assert_eq!(req.url, "https://example.com");
      assert_eq!(req.method.as_deref(), Some("POST"));
      assert_eq!(
         req.headers.as_ref().unwrap().get("content-type").unwrap(),
         "application/json"
      );
      assert_eq!(req.body.as_deref(), Some("hello"));
      assert_eq!(req.body_encoding, Some(BodyEncoding::Utf8));
      assert_eq!(req.timeout_ms, Some(5000));
      assert_eq!(req.request_id.as_deref(), Some("req-1"));
      assert!(req.max_retries.is_none());
   }

   #[test]
   fn test_fetch_request_minimal() {
      let json = serde_json::json!({"url": "https://example.com"});
      let req: FetchRequest = serde_json::from_value(json).unwrap();

      assert_eq!(req.url, "https://example.com");
      assert!(req.method.is_none());
      assert!(req.headers.is_none());
      assert!(req.body.is_none());
      assert!(req.body_encoding.is_none());
      assert!(req.timeout_ms.is_none());
      assert!(req.request_id.is_none());
      assert!(req.max_retries.is_none());
   }

   #[test]
   fn test_fetch_request_with_max_retries() {
      let json = serde_json::json!({
         "url": "https://example.com",
         "maxRetries": 5
      });

      let req: FetchRequest = serde_json::from_value(json).unwrap();

      assert_eq!(req.max_retries, Some(5));
   }

   #[test]
   fn test_fetch_request_missing_url_fails_deserialization() {
      let json = serde_json::json!({"method": "GET"});
      let result = serde_json::from_value::<FetchRequest>(json);

      assert!(result.is_err());
      let err_msg = result.unwrap_err().to_string();

      assert!(
         err_msg.contains("url"),
         "error should mention missing 'url' field: {err_msg}"
      );
   }

   #[test]
   fn test_fetch_response_metadata_serializes_camel_case() {
      let meta = FetchResponseMetadata {
         status: 200,
         status_text: "OK".to_string(),
         headers: HashMap::from([("content-type".to_string(), vec!["text/html".to_string()])]),
         url: "https://example.com".to_string(),
         redirected: false,
         retry_count: 0,
      };

      let json = serde_json::to_value(&meta).unwrap();

      assert_eq!(json["status"], 200);
      assert_eq!(json["statusText"], "OK");
      assert_eq!(json["url"], "https://example.com");
      assert_eq!(json["redirected"], false);
      assert_eq!(json["retryCount"], 0);
      assert!(json["headers"]["content-type"].is_array());
      // Metadata has no body or bodyEncoding fields
      assert!(json.get("body").is_none());
      assert!(json.get("bodyEncoding").is_none());
   }

   #[test]
   fn test_fetch_response_metadata_retry_count_serializes() {
      let meta = FetchResponseMetadata {
         status: 200,
         status_text: "OK".to_string(),
         headers: HashMap::new(),
         url: "https://example.com".to_string(),
         redirected: false,
         retry_count: 3,
      };

      let json = serde_json::to_value(&meta).unwrap();

      assert_eq!(json["retryCount"], 3);
   }

   #[test]
   fn test_fetch_request_invalid_body_encoding_fails_deserialization() {
      let json = serde_json::json!({
         "url": "https://example.com",
         "body": "hello",
         "bodyEncoding": "gzip"
      });

      let result = serde_json::from_value::<FetchRequest>(json);

      assert!(result.is_err());
   }
}
