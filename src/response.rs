//! Native response type for backend (non-IPC) HTTP requests.

/// Native Rust response type for backend (non-IPC) callers.
///
/// Unlike `ExecuteResult` (the IPC-oriented type in `types.rs`), this type exposes
/// native `reqwest` types (`StatusCode`, `HeaderMap`, `Url`) rather than
/// IPC-serializable primitives. Created by
/// [`RequestBuilder::send`](crate::request::RequestBuilder::send).
#[derive(Debug)]
pub struct Response {
   status: reqwest::StatusCode,
   headers: reqwest::header::HeaderMap,
   url: url::Url,
   redirected: bool,
   body: Vec<u8>,
   retry_count: u32,
}

impl Response {
   pub(crate) fn new(
      status: reqwest::StatusCode,
      headers: reqwest::header::HeaderMap,
      url: url::Url,
      redirected: bool,
      body: Vec<u8>,
      retry_count: u32,
   ) -> Self {
      Self {
         status,
         headers,
         url,
         redirected,
         body,
         retry_count,
      }
   }

   /// Returns the HTTP status code.
   pub fn status(&self) -> reqwest::StatusCode {
      self.status
   }

   /// Returns the response headers.
   pub fn headers(&self) -> &reqwest::header::HeaderMap {
      &self.headers
   }

   /// Returns the final URL after any redirects.
   pub fn url(&self) -> &url::Url {
      &self.url
   }

   /// Returns `true` if the response was the result of a redirect.
   pub fn redirected(&self) -> bool {
      self.redirected
   }

   /// Returns the response body as a byte slice.
   pub fn body(&self) -> &[u8] {
      &self.body
   }

   /// Consumes the response and returns the body bytes.
   pub fn into_body(self) -> Vec<u8> {
      self.body
   }

   /// Returns the number of retry attempts before this response (0 = no retries).
   pub fn retry_count(&self) -> u32 {
      self.retry_count
   }

   /// Convenience: decode body as UTF-8 string.
   pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
      std::str::from_utf8(&self.body)
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use reqwest::header::{HeaderMap, HeaderValue};

   fn sample_response() -> Response {
      let mut headers = HeaderMap::new();

      headers.insert("content-type", HeaderValue::from_static("application/json"));

      Response::new(
         reqwest::StatusCode::OK,
         headers,
         url::Url::parse("https://example.com/data").unwrap(),
         false,
         b"hello world".to_vec(),
         0,
      )
   }

   #[test]
   fn test_status() {
      let resp = sample_response();

      assert_eq!(resp.status(), reqwest::StatusCode::OK);
   }

   #[test]
   fn test_headers() {
      let resp = sample_response();

      assert_eq!(
         resp.headers().get("content-type").unwrap(),
         "application/json"
      );
   }

   #[test]
   fn test_url() {
      let resp = sample_response();

      assert_eq!(resp.url().as_str(), "https://example.com/data");
   }

   #[test]
   fn test_redirected_false() {
      let resp = sample_response();

      assert!(!resp.redirected());
   }

   #[test]
   fn test_redirected_true() {
      let resp = Response::new(
         reqwest::StatusCode::OK,
         HeaderMap::new(),
         url::Url::parse("https://example.com/final").unwrap(),
         true,
         Vec::new(),
         0,
      );

      assert!(resp.redirected());
   }

   #[test]
   fn test_body() {
      let resp = sample_response();

      assert_eq!(resp.body(), b"hello world");
   }

   #[test]
   fn test_into_body() {
      let resp = sample_response();

      assert_eq!(resp.into_body(), b"hello world");
   }

   #[test]
   fn test_retry_count() {
      let resp = Response::new(
         reqwest::StatusCode::OK,
         HeaderMap::new(),
         url::Url::parse("https://example.com").unwrap(),
         false,
         Vec::new(),
         3,
      );

      assert_eq!(resp.retry_count(), 3);
   }

   #[test]
   fn test_text_valid_utf8() {
      let resp = sample_response();

      assert_eq!(resp.text().unwrap(), "hello world");
   }

   #[test]
   fn test_text_invalid_utf8() {
      let resp = Response::new(
         reqwest::StatusCode::OK,
         HeaderMap::new(),
         url::Url::parse("https://example.com").unwrap(),
         false,
         vec![0xFF, 0xFE],
         0,
      );

      assert!(resp.text().is_err());
   }

   #[test]
   fn test_empty_body() {
      let resp = Response::new(
         reqwest::StatusCode::NO_CONTENT,
         HeaderMap::new(),
         url::Url::parse("https://example.com").unwrap(),
         false,
         Vec::new(),
         0,
      );

      assert!(resp.body().is_empty());
      assert_eq!(resp.text().unwrap(), "");
   }
}
