use deno_core::serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum WebViewStatus {
    Initialized,
    WindowCreated,
}
