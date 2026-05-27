use serde::{Deserialize, Serialize};
use reqwest::Client;
use std::path::Path;
use tokio::process::Command as TokioCommand;
use crate::state::config_models::AiSettings;
use crate::state::config_models::UniquenessSettings;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize)]
struct DubbingRequest {
    text: String,
    language: String,
    voice_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VoxCPMResponse {
    audio_url: Option<String>,
    error: Option<String>,
}

pub async fn run_ai_dubbing(
    app: &tauri::AppHandle,
    video_path: &Path,
    srt_content: &str,
    ai: &AiSettings,
    uniqueness: &UniquenessSettings,
) -> Result<String, String> {
    let client = Client::new();
    
    // 1. Translate the transcript line-by-line or in blocks using Deepseek
    let translation_prompt = format!(
        "You are a professional translator. Translate the following English transcript into natural, high-impact {}, \
        optimized for viral social media content. Maintain the tone and slang. \
        Return ONLY the translated text, preserving the structure for line-by-line matching.",
        uniqueness.target_language
    );

    let translated_text = crate::ai::deepseek::generate_translation(
        &ai.deepseek_api_key, 
        srt_content, 
        &translation_prompt
    ).await?;

    // 2. Generate Dubbed Audio using VoxCPM
    // In a real implementation, we would loop through lines and sync, 
    // but here we provide the architectural framework for a full-file dub for simplicity.
    let response = client
        .post(format!("{}/generate", ai.voxcpm_api_url))
        .json(&DubbingRequest {
            text: translated_text,
            language: uniqueness.target_language.clone(),
            voice_id: uniqueness.voice_clone_id.clone(),
        })
        .send()
        .await
        .map_err(|e| format!("VoxCPM request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("VoxCPM error: {}", response.status()));
    }

    let result: VoxCPMResponse = response.json().await
        .map_err(|e| format!("Failed to parse VoxCPM response: {}", e))?;

    if let Some(error) = result.error {
        return Err(error);
    }

    let audio_url = result.audio_url.ok_or("No audio URL returned from VoxCPM")?;
    let dubbed_audio_path = video_path.with_extension("dubbed.mp3");
    
    // Download dubbed audio
    let audio_data = client.get(audio_url).send().await
        .map_err(|e| format!("Failed to download dubbed audio: {}", e))?
        .bytes().await
        .map_err(|e| format!("Failed to read audio data: {}", e))?;
    
    tokio::fs::write(&dubbed_audio_path, audio_data).await
        .map_err(|e| format!("Failed to save dubbed audio: {}", e))?;

    // 3. Sync and Merge using FFmpeg
    let paths_manager = app.state::<crate::paths::PathsManager>();
    let bin_dir = paths_manager.bin_dir();
    let separator = if cfg!(windows) { ';' } else { ':' };
    let path_env = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);

    let output_path = video_path.with_extension("final_dubbed.mp4");

    // FFmpeg command to replace audio and apply atempo to match durations if needed
    // Simplified: replaces audio stream
    let status = TokioCommand::new("ffmpeg")
        .env("PATH", &new_path)
        .args(&[
            "-i", video_path.to_str().unwrap(),
            "-i", dubbed_audio_path.to_str().unwrap(),
            "-c:v", "copy",
            "-map", "0:v:0",
            "-map", "1:a:0",
            "-shortest",
            output_path.to_str().unwrap(),
            "-y"
        ])
        .status()
        .await
        .map_err(|e| format!("FFmpeg merge failed: {}", e))?;

    if status.success() {
        let _ = tokio::fs::remove_file(dubbed_audio_path).await;
        Ok(output_path.to_str().unwrap().to_string())
    } else {
        Err("FFmpeg merge failed".to_string())
    }
}
