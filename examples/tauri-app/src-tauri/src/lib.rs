use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;
use tauri_plugin_http_client::HttpClientExt;

/// Response shape returned to the frontend from the Rust backend request.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackendResponse {
   status: u16,
   status_text: String,
   headers: HashMap<String, String>,
   url: String,
   redirected: bool,
   body: String,
}

/// Demonstrates the Rust backend API: makes an HTTP request through the
/// plugin's security pipeline from Rust, then returns the result to the
/// frontend via IPC.
#[tauri::command]
async fn fetch_from_rust(
   app: tauri::AppHandle,
   url: String,
) -> Result<BackendResponse, String> {
   let resp = app
      .http_client()
      .get(&url)
      .header("Accept", "application/json")
      .header("X-Requested-From", "rust-backend")
      .send()
      .await
      .map_err(|e| e.to_string())?;

   // Note: multi-value headers (e.g. Set-Cookie) are collapsed to last value.
   let mut headers = HashMap::new();

   for (name, value) in resp.headers() {
      if let Ok(v) = value.to_str() {
         headers.insert(name.as_str().to_string(), v.to_string());
      }
   }

   let status_text = resp
      .status()
      .canonical_reason()
      .unwrap_or("")
      .to_string();

   Ok(BackendResponse {
      status: resp.status().as_u16(),
      status_text,
      headers,
      url: resp.url().to_string(),
      redirected: resp.redirected(),
      body: resp.text().map(|s| s.to_string()).unwrap_or_default(),
   })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
   tauri::Builder::default()
      .plugin(
         tauri_plugin_http_client::Builder::new()
            .allowed_domains(["httpbin.org", "*.httpbin.org"])
            .default_timeout(Duration::from_secs(30))
            .max_response_body_size(5 * 1024 * 1024)
            .build(),
      )
      .invoke_handler(tauri::generate_handler![fetch_from_rust])
      .run(tauri::generate_context!())
      .expect("error while running tauri application");
}
