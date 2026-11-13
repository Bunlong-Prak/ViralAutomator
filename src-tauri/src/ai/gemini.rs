use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde_json::json;
use std::path::Path;
use tokio::fs;

pub async fn transcribe_audio(api_key: &str, audio_path: &Path) -> Result<String, String> {
  if api_key.is_empty() {
    return Err("Gemini API key is required".to_string());
  }

  let audio_bytes = fs::read(audio_path)
    .await
    .map_err(|e| format!("Failed to read audio file: {}", e))?;

  let base64_audio = general_purpose::STANDARD.encode(audio_bytes);

  let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent?key={}",
        api_key
    );

  let client = Client::new();

  let request_body = json!({
      "contents": [{
          "parts": [
              {
                  "text": "Transcribe the following audio accurately. Output strictly in valid SubRip (SRT) subtitle format without any conversational text or markdown code blocks around it."
              },
              {
                  "inline_data": {
                      "mime_type": "audio/mp3",
                      "data": base64_audio
                  }
              }
          ]
      }]
  });

  let response = client
    .post(&url)
    .header("Content-Type", "application/json")
    .json(&request_body)
    .send()
    .await
    .map_err(|e| format!("Gemini API request failed: {}", e))?;

  if !response.status().is_success() {
    let err_text = response
      .text()
      .await
      .unwrap_or_else(|_| "Unknown error".to_string());
    return Err(format!("Gemini API error: {}", err_text));
  }

  let response_json: serde_json::Value = response
    .json()
    .await
    .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;

  let raw_transcription = response_json["candidates"][0]["content"]["parts"][0]["text"]
    .as_str()
    .ok_or_else(|| "Failed to extract transcription from Gemini response".to_string())?
    .trim();

  let mut transcription = raw_transcription;
  if let Some(stripped) = transcription.strip_prefix("```srt") {
    transcription = stripped.trim_start();
  } else if let Some(stripped) = transcription.strip_prefix("```") {
    transcription = stripped.trim_start();
  }

  if let Some(stripped) = transcription.strip_suffix("```") {
    transcription = stripped.trim_end();
  }

  Ok(transcription.to_string())
}
