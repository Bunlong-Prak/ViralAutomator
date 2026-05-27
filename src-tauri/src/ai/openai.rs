use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use reqwest::multipart;

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptionResponse {
    pub text: String,
}

pub async fn transcribe_audio(
    api_key: &str,
    audio_path: &Path,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    
    let file_content = fs::read(audio_path).await
        .map_err(|e| format!("Failed to read audio file: {}", e))?;
    
    let part = multipart::Part::bytes(file_content)
        .file_name("audio.mp3")
        .mime_str("audio/mpeg")
        .map_err(|e| e.to_string())?;

    let form = multipart::Form::new()
        .part("file", part)
        .text("model", "whisper-1")
        .text("response_format", "srt");

    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI API error: {} - {}", status, error_body));
    }

    let srt_content = response.text().await
        .map_err(|e| format!("Failed to read response text: {}", e))?;

    Ok(srt_content)
}
