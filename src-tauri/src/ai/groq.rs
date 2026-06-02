use reqwest::multipart;
use serde::Deserialize;

use std::path::Path;
use tokio::fs;

pub async fn transcribe_audio(api_key: &str, audio_path: &Path) -> Result<String, String> {
  let client = reqwest::Client::new();

  let file_content = fs::read(audio_path)
    .await
    .map_err(|e| format!("Failed to read audio file: {}", e))?;

  let part = multipart::Part::bytes(file_content)
    .file_name("audio.mp3")
    .mime_str("audio/mpeg")
    .map_err(|e| e.to_string())?;

  let form = multipart::Form::new()
    .part("file", part)
    .text("model", "whisper-large-v3")
    .text("response_format", "verbose_json");

  let response = client
    .post("https://api.groq.com/openai/v1/audio/transcriptions")
    .header("Authorization", format!("Bearer {}", api_key))
    .multipart(form)
    .send()
    .await
    .map_err(|e| format!("Request failed: {}", e))?;

  if !response.status().is_success() {
    let status = response.status();
    let error_body = response.text().await.unwrap_or_default();
    return Err(format!("Groq API error: {} - {}", status, error_body));
  }

  let text = response
    .text()
    .await
    .map_err(|e| format!("Failed to read response text: {}", e))?;

  #[derive(Deserialize)]
  struct GroqVerboseJson {
    segments: Option<Vec<GroqSegment>>,
  }

  #[derive(Deserialize)]
  struct GroqSegment {
    start: f64,
    end: f64,
    text: String,
  }

  let data: GroqVerboseJson = serde_json::from_str(&text)
    .map_err(|e| format!("Failed to parse Groq JSON: {} - {}", e, text))?;

  let mut srt = String::new();
  if let Some(segments) = data.segments {
    for (i, segment) in segments.iter().enumerate() {
      let start_hours = (segment.start / 3600.0).floor() as u64;
      let start_minutes = ((segment.start % 3600.0) / 60.0).floor() as u64;
      let start_secs = (segment.start % 60.0).floor() as u64;
      let start_millis = (segment.start.fract() * 1000.0).round() as u64;

      let end_hours = (segment.end / 3600.0).floor() as u64;
      let end_minutes = ((segment.end % 3600.0) / 60.0).floor() as u64;
      let end_secs = (segment.end % 60.0).floor() as u64;
      let end_millis = (segment.end.fract() * 1000.0).round() as u64;

      srt.push_str(&format!("{}\n", i + 1));
      srt.push_str(&format!(
        "{:02}:{:02}:{:02},{:03} --> {:02}:{:02}:{:02},{:03}\n",
        start_hours,
        start_minutes,
        start_secs,
        start_millis,
        end_hours,
        end_minutes,
        end_secs,
        end_millis
      ));
      srt.push_str(&format!("{}\n\n", segment.text.trim()));
    }
  }

  Ok(srt)
}
