pub mod captions;
pub mod deepseek;
pub mod dubbing;
pub mod gemini;
pub mod groq;
pub mod openai;
pub mod shorts;

use crate::ai::shorts::VideoSegment;
use crate::state::config_models::AiSettings;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiPlatformMetadata {
  pub hook_text: String,
  pub youtube_title: String,
  pub youtube_description: String,
  pub tiktok_caption: String,
  pub instagram_caption: String,
  pub hashtags: Vec<String>,
}

impl MultiPlatformMetadata {
  pub fn fallback(title: &str) -> Self {
    let clean_title = if title.trim().is_empty() {
      "Viral Clip".to_string()
    } else {
      title.trim().to_string()
    };
    let hook = clean_title
      .split_whitespace()
      .take(5)
      .collect::<Vec<_>>()
      .join(" ")
      .to_uppercase();
    Self {
      hook_text: hook,
      youtube_title: clean_title.clone(),
      youtube_description: format!("{clean_title}\n\n#shorts #viral #trending"),
      tiktok_caption: format!("{clean_title} #fyp #viral #trending"),
      instagram_caption: format!(
        "{clean_title}\n\nSave this & follow for more!\n\n#reels #viral #trending"
      ),
      hashtags: vec![
        "shorts".into(),
        "fyp".into(),
        "reels".into(),
        "viral".into(),
      ],
    }
  }
}

pub fn has_any_llm_key(ai: &AiSettings) -> bool {
  gemini::resolve_gemini_key(&ai.gemini_api_key).is_some()
    || !ai.deepseek_api_key.trim().is_empty()
    || !ai.groq_api_key.trim().is_empty()
}

pub fn has_valid_caption_key(ai: &AiSettings, provider: &str) -> bool {
  match provider {
    "gemini" => gemini::resolve_gemini_key(&ai.gemini_api_key).is_some(),
    "groq" => {
      !ai.groq_api_key.trim().is_empty()
        || (!ai.deepseek_api_key.trim().is_empty() && ai.deepseek_api_key.starts_with("gsk_"))
    }
    "openai" => !ai.openai_api_key.trim().is_empty(),
    _ => {
      gemini::resolve_gemini_key(&ai.gemini_api_key).is_some()
        || !ai.groq_api_key.trim().is_empty()
        || !ai.openai_api_key.trim().is_empty()
    }
  }
}

/// Gemini-first viral clip segment detection, falling back to DeepSeek/Groq if Gemini is unconfigured or fails.
pub async fn identify_viral_segments_auto(
  ai: &AiSettings,
  transcript: &str,
  num_clips: usize,
) -> Result<Vec<VideoSegment>, String> {
  let mut gemini_err = None;
  if let Some(gemini_key) = gemini::resolve_gemini_key(&ai.gemini_api_key) {
    match gemini::identify_viral_segments(&gemini_key, transcript, num_clips).await {
      Ok(segments) if !segments.is_empty() => return Ok(segments),
      Ok(_) => gemini_err = Some("Gemini returned zero viral segments".to_string()),
      Err(e) => {
        tracing::warn!("Gemini viral segment detection failed: {}", e);
        gemini_err = Some(e);
      }
    }
  }

  let fallback_key = if !ai.deepseek_api_key.trim().is_empty() {
    Some(ai.deepseek_api_key.as_str())
  } else if !ai.groq_api_key.trim().is_empty() {
    Some(ai.groq_api_key.as_str())
  } else {
    None
  };

  if let Some(key) = fallback_key {
    return shorts::identify_viral_segments(key, transcript, num_clips).await;
  }

  Err(
    gemini_err.unwrap_or_else(|| {
      "No AI API key configured (Gemini, DeepSeek, or Groq required)".to_string()
    }),
  )
}

/// Gemini-first multi-platform viral metadata generation (YT Shorts, TikTok, IG Reels + 3s hook).
pub async fn generate_metadata_auto(
  ai: &AiSettings,
  source_title: &str,
  transcript_excerpt: &str,
) -> Result<MultiPlatformMetadata, String> {
  if let Some(gemini_key) = gemini::resolve_gemini_key(&ai.gemini_api_key) {
    match gemini::generate_multi_platform_metadata(&gemini_key, source_title, transcript_excerpt)
      .await
    {
      Ok(meta) => return Ok(meta),
      Err(e) => {
        tracing::warn!("Gemini metadata generation failed: {}", e);
      }
    }
  }

  let fallback_key = if !ai.deepseek_api_key.trim().is_empty() {
    Some(ai.deepseek_api_key.as_str())
  } else if !ai.groq_api_key.trim().is_empty() {
    Some(ai.groq_api_key.as_str())
  } else {
    None
  };

  if let Some(key) = fallback_key {
    let (title, desc) = deepseek::generate_metadata(key, source_title).await?;
    let mut meta = MultiPlatformMetadata::fallback(&title);
    meta.youtube_title = title.clone();
    meta.youtube_description = desc.clone();
    meta.tiktok_caption = format!("{title} #fyp #viral");
    meta.instagram_caption = format!("{desc}\n\n#reels #viral");
    return Ok(meta);
  }

  Err("No AI API key configured for metadata generation".to_string())
}

/// Gemini-first translation / hook rewriting, falling back to DeepSeek/Groq.
pub async fn generate_translation_auto(
  ai: &AiSettings,
  text: &str,
  system_prompt: &str,
) -> Result<String, String> {
  if let Some(gemini_key) = gemini::resolve_gemini_key(&ai.gemini_api_key) {
    match gemini::generate_translation(&gemini_key, text, system_prompt).await {
      Ok(translated) => return Ok(translated),
      Err(e) => {
        tracing::warn!("Gemini translation failed: {}", e);
      }
    }
  }

  let fallback_key = if !ai.deepseek_api_key.trim().is_empty() {
    Some(ai.deepseek_api_key.as_str())
  } else if !ai.groq_api_key.trim().is_empty() {
    Some(ai.groq_api_key.as_str())
  } else {
    None
  };

  if let Some(key) = fallback_key {
    return deepseek::generate_translation(key, text, system_prompt).await;
  }

  Err("No AI API key configured for translation".to_string())
}
