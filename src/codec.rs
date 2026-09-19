use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::ffi::{CStr, CString, c_char};
unsafe extern "C" {
    fn journal_bridge(input: *const c_char) -> *mut c_char;
}
pub fn bridge(request: Value) -> Result<Value> {
    let input = CString::new(request.to_string())?;
    // The bridge consumes the input synchronously and returns a strdup allocation.
    let result = unsafe { journal_bridge(input.as_ptr()) };
    anyhow::ensure!(!result.is_null(), "macOS bridge returned no data");
    let bytes = unsafe { CStr::from_ptr(result).to_bytes().to_vec() };
    unsafe { libc::free(result.cast()) };
    let value: Value = serde_json::from_slice(&bytes)?;
    if let Some(error) = value["error"].as_str() {
        bail!("{error}")
    }
    Ok(value)
}
pub fn decode(blob: Option<&[u8]>) -> Result<String> {
    let Some(blob) = blob.filter(|b| !b.is_empty()) else {
        return Ok(String::new());
    };
    Ok(
        bridge(json!({"op":"decode", "text": STANDARD.encode(blob)}))?["text"]
            .as_str()
            .unwrap_or_default()
            .into(),
    )
}
pub fn encode(text: &str, markdown: bool) -> Result<(Vec<u8>, String)> {
    let request = if markdown {
        json!({"op":"styled","lines":crate::markdown::lines(text)?})
    } else {
        json!({"op":"encode","text":text})
    };
    let v = bridge(request)?;
    Ok((
        STANDARD.decode(v["data"].as_str().context("RTF data missing")?)?,
        v["text"].as_str().unwrap_or_default().into(),
    ))
}
pub fn inline(text: &str) -> Result<String> {
    crate::markdown::inline(text)
}
pub fn valid_rtf(data: &[u8]) -> Result<bool> {
    Ok(bridge(json!({"op":"decode","text":STANDARD.encode(data)}))?["valid"] == true)
}
