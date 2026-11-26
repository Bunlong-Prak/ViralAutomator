use crate::ai::shorts::VideoSegment;
use crate::ai::MultiPlatformMetadata;
use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tokio::fs;

const GEMINI_MODEL_FALLBACKS: &[&str] = &[
  "gemini-flash-latest",
  "gemini-3.5-flash",
  "gemini-3.5-flash-lite",
  "gemini-flash-lite-latest",
  "gemini-3.1-flash-lite",
];

pub fn resolve_gemini_key(api_key: &str) -> Option<String> {
  if !api_key.trim().is_empty() {
    return Some(api_key.trim().to_string());
  }
  crate::paths::load_env_var("GEMINI_API_KEY")
    .or_else(|| crate::paths::load_env_var("VITE_GEMINI_API_KEY"))
}

pub fn strip_markdown_fences(raw: &str) -> String {
  let mut text = raw.trim();
  for prefix in ["```json", "```srt", "```ass", "```"] {
    if let Some(stripped) = text.strip_prefix(prefix) {
      text = stripped.trim_start();
      break;
    }
  }
  if let Some(stripped) = text.strip_suffix("```") {
    text = stripped.trim_end();
  }
  text.to_string()
}

async fn call_gemini_with_fallback(
  api_key: &str,
  request_body: &serde_json::Value,
) -> Result<String, String> {
  let effective_key =
    resolve_gemini_key(api_key).ok_or_else(|| "Gemini API key is required".to_string())?;

  let is_oauth = effective_key.starts_with("ya29.") || effective_key.starts_with("Bearer ");
  let clean_token = effective_key
    .strip_prefix("Bearer ")
    .unwrap_or(&effective_key)
    .trim();

  let client = Client::builder()
    .timeout(Duration::from_secs(120))
    .build()
    .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

  let mut last_error = String::from("All Gemini models failed");

  for model in GEMINI_MODEL_FALLBACKS {
    let base_url = format!(
      "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
      model
    );

    // Up to 2 quick attempts per model on transient 503/429
    for attempt in 0..2 {
      let request_builder = if is_oauth {
        client
          .post(&base_url)
          .header("Authorization", format!("Bearer {}", clean_token))
      } else {
        let url = format!("{}?key={}", base_url, clean_token);
        client.post(&url)
      };

      let response = match request_builder
        .header("Content-Type", "application/json")
        .json(request_body)
        .send()
        .await
      {
        Ok(resp) => resp,
        Err(e) => {
          last_error = format!("Gemini ({model}) request failed: {e}");
          tracing::warn!("{}", last_error);
          break;
        }
      };

      let status = response.status();
      if status.is_success() {
        let response_json: serde_json::Value = response
          .json()
          .await
          .map_err(|e| format!("Failed to parse Gemini ({model}) JSON response: {e}"))?;

        let raw_text = response_json["candidates"][0]["content"]["parts"][0]["text"]
          .as_str()
          .ok_or_else(|| format!("Gemini ({model}) returned no text candidate"))?;

        return Ok(strip_markdown_fences(raw_text));
      }

      let err_text = response
        .text()
        .await
        .unwrap_or_else(|_| "Unknown error".to_string());
      last_error = format!("Gemini ({model}) HTTP {status}: {err_text}");

      // If auth error (401/403), don't retry other models
      if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(last_error);
      }

      if (status.as_u16() == 503 || status.as_u16() == 429) && attempt == 0 {
        tracing::warn!("{} — retrying in 1.5s...", last_error);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        continue;
      }

      tracing::warn!("{} — trying next fallback model...", last_error);
      break;
    }
  }

  Err(last_error)
}

pub async fn transcribe_audio(api_key: &str, audio_path: &Path) -> Result<String, String> {
  let audio_bytes = fs::read(audio_path)
    .await
    .map_err(|e| format!("Failed to read audio file: {}", e))?;

  let base64_audio = general_purpose::STANDARD.encode(audio_bytes);

  let request_body = json!({
    "contents": [{
      "parts": [
        {
          "text": "Transcribe the following audio accurately into valid SubRip (SRT) subtitle format. \
            Keep every subtitle cue punchy and short (3 to 6 words per cue, 1 to 2.5 seconds each) for viral short-form video captions. \
            Use exact HH:MM:SS,mmm --> HH:MM:SS,mmm timestamps. \
            Output ONLY raw valid SRT text with NO markdown code fences or extra commentary."
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

  call_gemini_with_fallback(api_key, &request_body).await
}

pub async fn identify_viral_segments(
  api_key: &str,
  transcript: &str,
  num_clips: usize,
) -> Result<Vec<VideoSegment>, String> {
  let prompt = format!(
    "You are an elite viral short-form video strategist for YouTube Shorts, TikTok, and Instagram Reels.\n\
    Analyze the timestamped SRT transcript below and select the top {num_clips} highest-retention viral segments.\n\
    Rules:\n\
    1. Each segment should be 25 to 58 seconds long (strictly under 60s for Shorts/Reels compatibility).\n\
    2. Start each clip on a strong hook, bold claim, conflict, or emotional peak — NEVER mid-sentence.\n\
    3. End on a clean payoff or cliffhanger.\n\
    4. Timestamps MUST be in HH:MM:SS format and fall strictly within the transcript duration.\n\
    5. Provide a `hook_text` (3 to 6 punchy ALL-CAPS words for the 3-second opening banner on screen).\n\
    6. Return strictly a JSON object: {{\"segments\": [{{\"start_time\": \"HH:MM:SS\", \"end_time\": \"HH:MM:SS\", \"virality_score\": 92, \"title\": \"Catchy Title\", \"hook_text\": \"STOP DOING THIS NOW\", \"reason\": \"Why viewers will watch to the end\"}}]}}.\n\n\
    Transcript:\n{transcript}"
  );

  let request_body = json!({
    "contents": [{
      "parts": [{ "text": prompt }]
    }],
    "generationConfig": {
      "responseMimeType": "application/json",
      "temperature": 0.4
    }
  });

  let clean_json = call_gemini_with_fallback(api_key, &request_body).await?;
  parse_segments_json(&clean_json)
}

pub fn parse_segments_json(clean_json: &str) -> Result<Vec<VideoSegment>, String> {
  let v: serde_json::Value =
    serde_json::from_str(clean_json).map_err(|e| format!("Failed to parse JSON: {}", e))?;

  let mut segments: Vec<VideoSegment> = if let Some(arr) = v
    .get("segments")
    .or_else(|| v.get("clips"))
    .or_else(|| v.get("viral_segments"))
    .or_else(|| v.get("data"))
    .and_then(|val| val.as_array())
    .or_else(|| {
      v.as_object()
        .and_then(|obj| obj.values().find_map(|val| val.as_array()))
    })
    .or_else(|| v.as_array())
  {
    serde_json::from_value(serde_json::Value::Array(arr.clone()))
      .map_err(|e| format!("Failed to deserialize segments: {e}"))?
  } else {
    return Err("Could not find segments array in JSON response".to_string());
  };

  // Sort highest virality score first so best clips publish first
  segments.sort_by_key(|b| std::cmp::Reverse(b.virality_score));
  Ok(segments)
}

pub async fn generate_multi_platform_metadata(
  api_key: &str,
  source_title: &str,
  transcript_excerpt: &str,
) -> Result<MultiPlatformMetadata, String> {
  let excerpt: String = transcript_excerpt.chars().take(2400).collect();
  let prompt = format!(
    "You are a $10k/month faceless short-form content growth strategist.\n\
    Create high-CTR, algorithm-optimized metadata for a vertical short-form video.\n\
    Source title: \"{source_title}\"\n\
    Transcript excerpt:\n{excerpt}\n\n\
    Return strictly a JSON object with these exact keys:\n\
    - \"hookText\": 3 to 6 punchy ALL-CAPS words for the 0-3s on-screen hook banner (no hashtags, no emojis)\n\
    - \"youtubeTitle\": Curiosity-driven YouTube Shorts title under 90 chars (no clickbait spam, high CTR)\n\
    - \"youtubeDescription\": 2-3 sentence SEO description with 3-5 relevant hashtags including #shorts\n\
    - \"tiktokCaption\": Punchy conversational TikTok caption with an engaging question + 4-5 niche hashtags\n\
    - \"instagramCaption\": Engaging Instagram Reels caption with a save/share CTA + 5-7 targeted hashtags\n\
    - \"hashtags\": array of 5 short hashtag strings without the # symbol"
  );

  let request_body = json!({
    "contents": [{
      "parts": [{ "text": prompt }]
    }],
    "generationConfig": {
      "responseMimeType": "application/json",
      "temperature": 0.7
    }
  });

  let clean_json = call_gemini_with_fallback(api_key, &request_body).await?;
  parse_multi_platform_metadata_json(&clean_json, source_title)
}

pub fn parse_multi_platform_metadata_json(
  clean_json: &str,
  fallback_title: &str,
) -> Result<MultiPlatformMetadata, String> {
  let start = clean_json.find('{').ok_or("No JSON object found")?;
  let end = clean_json.rfind('}').ok_or("No JSON object found")? + 1;
  let slice = &clean_json[start..end];

  #[derive(serde::Deserialize)]
  struct RawMeta {
    #[serde(alias = "hook_text", alias = "hookText", alias = "hook")]
    hook_text: Option<String>,
    #[serde(alias = "youtube_title", alias = "youtubeTitle", alias = "title")]
    youtube_title: Option<String>,
    #[serde(
      alias = "youtube_description",
      alias = "youtubeDescription",
      alias = "description"
    )]
    youtube_description: Option<String>,
    #[serde(alias = "tiktok_caption", alias = "tiktokCaption")]
    tiktok_caption: Option<String>,
    #[serde(alias = "instagram_caption", alias = "instagramCaption")]
    instagram_caption: Option<String>,
    hashtags: Option<Vec<String>>,
  }

  let parsed: RawMeta =
    serde_json::from_str(slice).map_err(|e| format!("Failed to parse metadata JSON: {e}"))?;

  let youtube_title = parsed
    .youtube_title
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| fallback_title.to_string());
  let hashtags = parsed
    .hashtags
    .unwrap_or_else(|| vec!["shorts".into(), "viral".into(), "fyp".into()])
    .into_iter()
    .map(|h| h.trim().trim_start_matches('#').to_string())
    .filter(|h| !h.is_empty())
    .collect::<Vec<_>>();
  let tag_str = hashtags
    .iter()
    .map(|h| format!("#{h}"))
    .collect::<Vec<_>>()
    .join(" ");

  let hook_text = parsed
    .hook_text
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| {
      youtube_title
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
    });

  let youtube_description = parsed
    .youtube_description
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| format!("{youtube_title}\n\n{tag_str} #shorts"));
  let tiktok_caption = parsed
    .tiktok_caption
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| format!("{youtube_title} {tag_str}"));
  let instagram_caption = parsed
    .instagram_caption
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| {
      format!("{youtube_title}\n\nSave this for later & follow for more!\n\n{tag_str} #reels")
    });

  Ok(MultiPlatformMetadata {
    hook_text,
    youtube_title,
    youtube_description,
    tiktok_caption,
    instagram_caption,
    hashtags,
  })
}

pub async fn generate_translation(
  api_key: &str,
  text: &str,
  system_prompt: &str,
) -> Result<String, String> {
  let request_body = json!({
    "system_instruction": {
      "parts": [{ "text": system_prompt }]
    },
    "contents": [{
      "parts": [{ "text": text }]
    }]
  });

  call_gemini_with_fallback(api_key, &request_body).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn strip_fences_handles_json_and_srt() {
    assert_eq!(
      strip_markdown_fences("```json\n{\"a\":1}\n```"),
      "{\"a\":1}"
    );
    assert_eq!(strip_markdown_fences("```srt\r\n1\r\n```"), "1");
    assert_eq!(strip_markdown_fences("plain text"), "plain text");
  }

  #[test]
  fn parse_segments_sorts_by_virality_desc() {
    let json = r#"{
      "segments": [
        {"start_time": "00:00:10", "end_time": "00:00:45", "virality_score": 78, "title": "Good Clip", "hook_text": "WAIT FOR THIS"},
        {"start_time": "00:02:00", "end_time": "00:02:45", "virality_score": 96, "title": "Best Clip", "hook_text": "STOP SCROLLING"}
      ]
    }"#;
    let segments = parse_segments_json(json).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].virality_score, 96);
    assert_eq!(segments[0].hook_text.as_deref(), Some("STOP SCROLLING"));
  }

  #[test]
  fn parse_multi_platform_metadata_builds_defaults() {
    let json = r##"{
      "hookText": "WHY 99% FAIL",
      "youtubeTitle": "The Brutal Truth About Scaling Content",
      "youtubeDescription": "Watch till the end #shorts",
      "tiktokCaption": "Agree or disagree? #contentcreator #fyp",
      "instagramCaption": "Save this! #reels",
      "hashtags": ["#viral", "growth"]
    }"##;
    let meta = parse_multi_platform_metadata_json(json, "Fallback").unwrap();
    assert_eq!(meta.hook_text, "WHY 99% FAIL");
    assert_eq!(meta.hashtags, vec!["viral", "growth"]);
    assert!(meta.tiktok_caption.contains("#fyp"));
  }
}
