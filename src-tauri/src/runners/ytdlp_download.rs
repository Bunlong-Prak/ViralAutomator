use crate::logging::LogStoreState;
use crate::models::download::DownloadSection;
use crate::models::{
  MediaDiagnosticPayload, MediaFatalPayload, MediaProgressComplete, ProgressEvent,
  MediaProgressStage,
};
use crate::state::config_models::AiSettings;
use std::path::{Path, PathBuf};
use tokio::process::Command as TokioCommand;
use crate::parsers::ytdlp_error::{DiagnosticMatcher, YtdlpErrorParser};
use crate::parsers::ytdlp_progress::{progress_category_for_track_type, YtdlpProgressParser};
use crate::runners::ytdlp_runner::{YtdlpCommandEvent, YtdlpRunner};
use crate::scheduling::download_pipeline::DownloadEntry;
use crate::scheduling::group_state::subscribe_group;
use std::fmt;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;

#[derive(Debug)]
pub enum YtdlpDownloadError {
  InvalidArguments(String),
  InvalidDiagnosticRules(String),
  SpawnFailed(String),
  RunnerError(String),
  NonZeroExit(i32),
  EventStreamEnded,
}

impl fmt::Display for YtdlpDownloadError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::InvalidArguments(e) => write!(f, "Invalid yt-dlp arguments: {e}"),
      Self::InvalidDiagnosticRules(e) => write!(f, "Invalid diagnostic rules: {e}"),
      Self::SpawnFailed(e) => write!(f, "Failed to spawn yt-dlp: {e}"),
      Self::RunnerError(e) => write!(f, "yt-dlp runner error: {e}"),
      Self::NonZeroExit(code) => write!(f, "yt-dlp exited with code {code}"),
      Self::EventStreamEnded => write!(f, "yt-dlp event stream ended unexpectedly"),
    }
  }
}

impl std::error::Error for YtdlpDownloadError {}

pub async fn run_ytdlp_download(
  app: AppHandle,
  entry: DownloadEntry,
) -> Result<(), YtdlpDownloadError> {
  let runner = YtdlpRunner::new(&app)
    .with_progress_args()
    .with_network_args(entry.overrides.as_ref())
    .with_auth_args(entry.overrides.as_ref())
    .with_subtitle_args(entry.overrides.as_ref(), entry.subtitle_inventory.as_ref())
    .with_sponsorblock_args(entry.overrides.as_ref())
    .with_format_args(&entry.format, entry.overrides.as_ref())
    .with_input_args(entry.overrides.as_ref());
  let output_args = match runner.output_args(&entry.format, entry.overrides.as_ref()) {
    Ok(args) => args,
    Err(err) => {
      emit_internal_fatal(
        &app,
        &entry,
        format!("Invalid download configuration: {err}"),
        Some(err.clone()),
      );
      return Err(YtdlpDownloadError::InvalidArguments(err));
    }
  };
  let runner = runner
    .with_args(output_args)
    .with_location_args(
      &entry.format.track_type,
      &entry.template_context,
      entry.overrides.as_ref(),
    )
    .with_url(&entry.url);

  static RULES_JSON: &str = include_str!("../diagnostic_rules.json");
  let matcher = match DiagnosticMatcher::from_json(RULES_JSON) {
    Ok(matcher) => matcher,
    Err(err) => {
      emit_internal_fatal(
        &app,
        &entry,
        format!("Invalid diagnostic rules: {err}"),
        Some(err.to_string()),
      );
      return Err(YtdlpDownloadError::InvalidDiagnosticRules(err.to_string()));
    }
  };

  let error_parser = YtdlpErrorParser::new(&entry.id, &entry.group_id, matcher);
  let partial_download_duration_secs = entry
    .overrides
    .as_ref()
    .and_then(|overrides| overrides.output.as_ref())
    .and_then(|output| output.partial_download.as_ref())
    .and_then(|partial| partial.section.as_ref())
    .and_then(download_section_duration_secs)
    .filter(|value| *value > 0.0);
  let mut progress_parser = YtdlpProgressParser::new(
    &entry.id,
    &entry.group_id,
    progress_category_for_track_type(&entry.format.track_type),
    partial_download_duration_secs,
  );

  let (mut rx, child) = match runner.spawn() {
    Ok(result) => result,
    Err(err) => {
      let message = format!("Failed to spawn yt-dlp: {err}");
      let _ = app.emit(
        "media_fatal",
        MediaFatalPayload::with_exit(entry.group_id.clone(), entry.id.clone(), 1, message),
      );
      return Err(YtdlpDownloadError::SpawnFailed(err));
    }
  };
  let mut cancel_rx = subscribe_group(&entry.group_id);
  let mut current_destination: Option<String> = None;

  loop {
    tokio::select! {
      event = rx.recv() => {
        let Some(event) = event else {
          break;
        };

        if is_cancelled_now(&cancel_rx) {
          tracing::info!("Cancelled processing for group_id {}", entry.group_id);
          let _ = child.kill_tree();
          return Ok(());
        }

        let log_state = app.state::<LogStoreState>();

        match event {
          YtdlpCommandEvent::Stdout(line) => {
            let line_str = String::from_utf8_lossy(&line);
            store_log_line(&line_str, &entry, log_state, &app);
            if let Some(ProgressEvent::Destination(dest)) = parse_progress_line(&line_str, &mut progress_parser, &app) {
               current_destination = Some(dest.destination.path);
            }
          }
          YtdlpCommandEvent::Stderr(line) => {
            let line_str = String::from_utf8_lossy(&line);
            store_log_line(&line_str, &entry, log_state, &app);
            if let Some(ProgressEvent::Destination(dest)) = parse_progress_line(&line_str, &mut progress_parser, &app) {
               current_destination = Some(dest.destination.path);
            }
            parse_error_line(&line_str, &error_parser, &app);
          }
          YtdlpCommandEvent::Terminated(term) => {
            if term.code == Some(0) {
              // Post-processing AI features
              if let Some(dest_path) = current_destination {
                let config_handle = app.state::<crate::SharedConfig>();
                let config = config_handle.load();
                
                let uniqueness = entry.overrides.as_ref()
                  .and_then(|o| o.output.as_ref())
                  .and_then(|o| o.video.as_ref())
                  .and_then(|v| v.uniqueness.as_ref());
                
                let mut uniqueness_config = config.output.video.uniqueness.clone();
                if let Some(u) = uniqueness {
                  if let Some(enabled) = u.enabled { uniqueness_config.enabled = enabled; }
                  if let Some(flip) = u.flip_horizontal { uniqueness_config.flip_horizontal = flip; }
                  if let Some(crop) = u.auto_crop { uniqueness_config.auto_crop = crop; }
                  if let Some(watermark) = u.watermark_text.as_ref() { uniqueness_config.watermark_text = watermark.clone(); }
                  if let Some(ai_caps) = u.ai_captions { uniqueness_config.ai_captions = ai_caps; }
                  if let Some(ai_meta) = u.ai_metadata { uniqueness_config.ai_metadata = ai_meta; }
                  if let Some(provider) = u.caption_provider.as_ref() { uniqueness_config.caption_provider = provider.clone(); }
                  if let Some(ai_shorts) = u.ai_shorts_mode { uniqueness_config.ai_shorts_mode = ai_shorts; }
                  if let Some(num) = u.num_shorts { uniqueness_config.num_shorts = num; }
                }

                if uniqueness_config.ai_shorts_mode && !config.ai.deepseek_api_key.is_empty() {
                    let _ = app.emit("media_progress_stage", MediaProgressStage {
                        id: entry.id.clone(),
                        group_id: entry.group_id.clone(),
                        stage: crate::models::ProgressStage::Finalizing,
                    });
                    
                    if let Err(e) = run_ai_shorts_extraction(&app, &dest_path, &config.ai, &uniqueness_config).await {
                        tracing::error!("AI Shorts extraction failed: {}", e);
                    }
                } else {
                    if uniqueness_config.ai_captions && !config.ai.openai_api_key.is_empty() {
                        let _ = app.emit("media_progress_stage", MediaProgressStage {
                            id: entry.id.clone(),
                            group_id: entry.group_id.clone(),
                            stage: crate::models::ProgressStage::Finalizing,
                        });
                        
                        if let Err(e) = run_ai_captions(&app, &dest_path, &config.ai, &uniqueness_config).await {
                            tracing::error!("AI Captions failed: {}", e);
                        }
                    }

                    if uniqueness_config.dubbing_enabled && !config.ai.deepseek_api_key.is_empty() {
                         // We need transcript for dubbing
                         let transcript = if let Ok(content) = run_ai_transcription(&app, &dest_path, &config.ai, &uniqueness_config).await {
                             content
                         } else {
                             "".to_string()
                         };
                         
                         if !transcript.is_empty() {
                            if let Err(e) = crate::ai::dubbing::run_ai_dubbing(&app, Path::new(&dest_path), &transcript, &config.ai, &uniqueness_config).await {
                                tracing::error!("Dubbing failed: {}", e);
                            }
                         }
                    }

                    if uniqueness_config.ai_metadata && !config.ai.deepseek_api_key.is_empty() {
                        if let Ok((title, desc)) = crate::ai::deepseek::generate_metadata(&config.ai.deepseek_api_key, &entry.id).await {
                            tracing::info!("AI Metadata generated: Title={}, Desc={}", title, desc);
                        }
                    }
                }
              }

              let _ = app.emit(
                "media_complete",
                MediaProgressComplete {
                  id: entry.id.clone(),
                  group_id: entry.group_id.clone(),
                },
              );
              return Ok(());
            }

            let exit = term.code.unwrap_or(1);
            let _ = app.emit(
              "media_fatal",
              MediaFatalPayload::with_exit(
                entry.group_id.clone(),
                entry.id.clone(),
                exit,
                format!("Download failed for group {}", entry.group_id),
              ),
            );
            return Err(YtdlpDownloadError::NonZeroExit(exit));
          }
          YtdlpCommandEvent::Error(err) => {
            let msg = format!("Download failed for group {}: {}", entry.group_id, err);
            emit_internal_fatal(&app, &entry, msg.clone(), Some(err.clone()));

            return Err(YtdlpDownloadError::RunnerError(err));
          }
        }
      }
      _ = cancel_rx.changed() => {
        if is_cancelled_now(&cancel_rx) {
          tracing::info!("Cancelled processing for group_id {}", entry.group_id);
          let _ = child.kill_tree();
          return Ok(());
        }
      }
    }
  }

  emit_internal_fatal(
    &app,
    &entry,
    "yt-dlp event stream ended unexpectedly".to_string(),
    None,
  );
  Err(YtdlpDownloadError::EventStreamEnded)
}

fn emit_internal_fatal(
  app: &AppHandle,
  entry: &DownloadEntry,
  message: String,
  details: Option<String>,
) {
  let _ = app.emit(
    "media_fatal",
    MediaFatalPayload::internal(entry.group_id.clone(), entry.id.clone(), message, details),
  );
}

fn is_cancelled_now(cancel_rx: &watch::Receiver<bool>) -> bool {
  !*cancel_rx.borrow()
}

fn store_log_line(
  line: &str,
  entry: &DownloadEntry,
  log_state: State<LogStoreState>,
  app: &AppHandle,
) {
  if line.is_empty() || line.starts_with("RAW") {
    return;
  }
  let mut store = log_state.write();
  store.append_line(app, &entry.group_id, line);
}

fn parse_progress_line(line: &str, progress_parser: &mut YtdlpProgressParser, app: &AppHandle) -> Option<ProgressEvent> {
  let progress_events = progress_parser.parse_line(line);
  let mut destination = None;

  for progress_event in progress_events {
    match progress_event {
      ProgressEvent::Destination(ref d) => {
        destination = Some(progress_event.clone());
        app.emit("media_destination", d).ok();
      }
      ProgressEvent::Progress(progress) => {
        app.emit("media_progress", progress).ok();
      }
      ProgressEvent::StageChange(progress) => {
        app.emit("media_progress_stage", progress).ok();
      }
    }
  }
  destination
}

fn parse_error_line(line: &str, error_parser: &YtdlpErrorParser, app: &AppHandle) {
  if let Some(event) = error_parser.parse_line(line) {
    app
      .emit(
        "media_diagnostic",
        MediaDiagnosticPayload::from_diagnostic_event(event),
      )
      .ok();
  }
}

fn download_section_duration_secs(section: &DownloadSection) -> Option<f64> {
  let start = parse_clock_secs(&section.start)?;
  let end = parse_clock_secs(&section.end)?;
  (end > start).then_some(end - start)
}

fn parse_clock_secs(value: &str) -> Option<f64> {
  let mut total = 0.0;
  let parts = value.trim().split(':').collect::<Vec<_>>();
  if parts.len() != 3 {
    return None;
  }

  for (index, part) in parts.iter().enumerate() {
    let component = part.trim().parse::<f64>().ok()?;
    total += component
      * match index {
        0 => 3600.0,
        1 => 60.0,
        2 => 1.0,
        _ => unreachable!(),
      };
  }

  Some(total)
}

async fn run_ai_shorts_extraction(
  app: &AppHandle, 
  dest_path: &str, 
  ai: &AiSettings, 
  uniqueness: &crate::state::config_models::UniquenessSettings
) -> Result<(), String> {
  let paths_manager = app.state::<crate::paths::PathsManager>();
  let bin_dir = paths_manager.bin_dir();
  let separator = if cfg!(windows) { ';' } else { ':' };
  let path_env = std::env::var("PATH").unwrap_or_default();
  let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);

  let video_path = PathBuf::from(dest_path);
  let audio_path = video_path.with_extension("mp3");
  let srt_path = video_path.with_extension("srt");
  
  // 1. Extract audio for transcription
  let _ = TokioCommand::new("ffmpeg")
    .env("PATH", &new_path)
    .args(&[
      "-i", dest_path,
      "-vn",
      "-ar", "16000",
      "-ac", "1",
      "-c:a", "libmp3lame",
      audio_path.to_str().ok_or("Invalid audio path")?,
      "-y"
    ])
    .status()
    .await;

  // 2. Transcribe
  let srt_content = if uniqueness.caption_provider == "groq" {
    crate::ai::groq::transcribe_audio(&ai.groq_api_key, &audio_path).await?
  } else {
    crate::ai::openai::transcribe_audio(&ai.openai_api_key, &audio_path).await?
  };
  tokio::fs::write(&srt_path, &srt_content).await.map_err(|e| e.to_string())?;

  // 3. Identify segments using Deepseek
  let segments = crate::ai::shorts::identify_viral_segments(&ai.deepseek_api_key, &srt_content, uniqueness.num_shorts).await?;
  
  // 4. Extract and process each segment
  for (i, segment) in segments.iter().enumerate() {
    let output_name = format!("{}_short_{}.mp4", video_path.file_stem().unwrap().to_str().unwrap(), i + 1);
    let output_path = video_path.parent().unwrap().join(output_name);
    
    // Create segment-specific SRT (simplified: just use the full SRT, FFmpeg subtitles filter handles timestamps)
    // For vertical crop: crop=ih*(9/16):ih
    let mut filters = vec!["crop=ih*(9/16):ih".to_string()];
    if uniqueness.flip_horizontal {
        filters.push("hflip".to_string());
    }
    
    let escaped_srt = srt_path.to_str().unwrap().replace("\\", "/").replace(":", "\\:");
    filters.push(format!("subtitles='{}':force_style='{}'", escaped_srt, ai.caption_style));
    
    let filter_chain = filters.join(",");

    let args = vec![
      "-ss".to_string(), segment.start_time.clone(),
      "-to".to_string(), segment.end_time.clone(),
      "-i".to_string(), dest_path.to_string(),
      "-vf".to_string(), filter_chain,
      "-c:a".to_string(), "copy".to_string(),
      output_path.to_str().unwrap().to_string(),
      "-y".to_string()
    ];

    let _ = TokioCommand::new("ffmpeg")
      .env("PATH", &new_path)
      .args(&args)
      .status()
      .await;

    // Optional: If dubbing is enabled, dub each short
    if uniqueness.dubbing_enabled && !ai.deepseek_api_key.is_empty() {
        let _ = crate::ai::dubbing::run_ai_dubbing(&app, &output_path, &srt_content, &ai, &uniqueness).await;
    }
  }

  // Cleanup
  let _ = tokio::fs::remove_file(audio_path).await;
  let _ = tokio::fs::remove_file(srt_path).await;

  Ok(())
}

async fn run_ai_transcription(
  app: &AppHandle, 
  dest_path: &str, 
  ai: &AiSettings, 
  uniqueness: &crate::state::config_models::UniquenessSettings
) -> Result<String, String> {
  let paths_manager = app.state::<crate::paths::PathsManager>();
  let bin_dir = paths_manager.bin_dir();
  let separator = if cfg!(windows) { ';' } else { ':' };
  let path_env = std::env::var("PATH").unwrap_or_default();
  let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);

  let video_path = PathBuf::from(dest_path);
  let audio_path = video_path.with_extension("mp3_tmp");
  
  let _ = TokioCommand::new("ffmpeg")
    .env("PATH", &new_path)
    .args(&["-i", dest_path, "-vn", "-ar", "16000", "-ac", "1", "-c:a", "libmp3lame", audio_path.to_str().unwrap(), "-y"])
    .status()
    .await;

  let srt_content = if uniqueness.caption_provider == "groq" {
    crate::ai::groq::transcribe_audio(&ai.groq_api_key, &audio_path).await?
  } else {
    crate::ai::openai::transcribe_audio(&ai.openai_api_key, &audio_path).await?
  };
  
  let _ = tokio::fs::remove_file(audio_path).await;
  Ok(srt_content)
}

async fn run_ai_captions(
  app: &AppHandle, 
  dest_path: &str, 
  ai: &AiSettings, 
  uniqueness: &crate::state::config_models::UniquenessSettings
) -> Result<(), String> {
  let paths_manager = app.state::<crate::paths::PathsManager>();
  let bin_dir = paths_manager.bin_dir();
  let separator = if cfg!(windows) { ';' } else { ':' };
  let path_env = std::env::var("PATH").unwrap_or_default();
  let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);

  let video_path = PathBuf::from(dest_path);
  let audio_path = video_path.with_extension("mp3");
  let srt_path = video_path.with_extension("srt");
  
  // 1. Extract audio
  let status = TokioCommand::new("ffmpeg")
    .env("PATH", &new_path)
    .args(&[
      "-i", dest_path,
      "-vn",
      "-ar", "16000",
      "-ac", "1",
      "-c:a", "libmp3lame",
      audio_path.to_str().ok_or("Invalid audio path")?,
      "-y"
    ])
    .status()
    .await
    .map_err(|e| format!("FFmpeg extract failed: {}", e))?;
    
  if !status.success() {
    return Err("FFmpeg extract failed".to_string());
  }

  // 2. Transcribe
  let srt_content = if uniqueness.caption_provider == "groq" {
    crate::ai::groq::transcribe_audio(&ai.groq_api_key, &audio_path).await?
  } else {
    crate::ai::openai::transcribe_audio(&ai.openai_api_key, &audio_path).await?
  };
  tokio::fs::write(&srt_path, srt_content).await.map_err(|e| e.to_string())?;

  // 3. Burn captions
  let final_path = video_path.with_extension("final.mp4");
  // Escape for FFmpeg subtitles filter
  let escaped_srt = srt_path.to_str().unwrap().replace("\\", "/").replace(":", "\\:");
  let filter = format!("subtitles='{}':force_style='{}'", escaped_srt, ai.caption_style);
  
  let status = TokioCommand::new("ffmpeg")
    .env("PATH", &new_path)
    .args(&[
      "-i", dest_path,
      "-vf", &filter,
      "-c:a", "copy",
      final_path.to_str().ok_or("Invalid final path")?,
      "-y"
    ])
    .status()
    .await
    .map_err(|e| format!("FFmpeg burn failed: {}", e))?;

  if status.success() {
    // Replace original with final
    let _ = tokio::fs::rename(final_path, video_path).await;
  }
  
  // Cleanup
  let _ = tokio::fs::remove_file(audio_path).await;
  let _ = tokio::fs::remove_file(srt_path).await;

  Ok(())
}
