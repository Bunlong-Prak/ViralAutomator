use crate::logging::LogStoreState;
use crate::models::download::DownloadSection;
use crate::models::{
  MediaDiagnosticPayload, MediaFatalPayload, MediaProgressComplete, MediaProgressStage,
  ProgressEvent,
};
use crate::parsers::ytdlp_error::{DiagnosticMatcher, YtdlpErrorParser};
use crate::parsers::ytdlp_progress::{progress_category_for_track_type, YtdlpProgressParser};
use crate::runners::ytdlp_runner::{YtdlpCommandEvent, YtdlpRunner};
use crate::scheduling::download_pipeline::DownloadEntry;
use crate::scheduling::group_state::subscribe_group;
use crate::state::config_models::AiSettings;
use std::fmt;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::process::Command as TokioCommand;
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
                  if let Some(dubbing) = u.dubbing_enabled { uniqueness_config.dubbing_enabled = dubbing; }
                  if let Some(target) = u.target_language.as_ref() { uniqueness_config.target_language = target.clone(); }
                  if let Some(voice) = u.voice_clone_id.as_ref() { uniqueness_config.voice_clone_id = voice.clone(); }
                }

                if uniqueness_config.ai_shorts_mode && !config.ai.deepseek_api_key.is_empty() {
                    let _ = app.emit("media_progress_stage", MediaProgressStage {
                        id: entry.id.clone(),
                        group_id: entry.group_id.clone(),
                        stage: crate::models::ProgressStage::Finalizing,
                    });

                    if let Err(e) = run_ai_shorts_extraction(&app, &dest_path, &config.ai, &uniqueness_config).await {
                        tracing::error!("AI Shorts extraction failed: {}", e);
                        return Err(YtdlpDownloadError::RunnerError(format!("AI Shorts extraction failed: {}", e)));
                    }
                } else {
                    let has_valid_caption_key = if uniqueness_config.caption_provider == "groq" {
                        !config.ai.groq_api_key.is_empty() || (!config.ai.deepseek_api_key.is_empty() && config.ai.deepseek_api_key.starts_with("gsk_"))
                    } else {
                        !config.ai.openai_api_key.is_empty()
                    };

                    if uniqueness_config.ai_captions {
                        if !has_valid_caption_key {
                            return Err(YtdlpDownloadError::RunnerError(format!("AI Captions is enabled, but the API key for '{}' is missing.", uniqueness_config.caption_provider)));
                        }
                        let _ = app.emit("media_progress_stage", MediaProgressStage {
                            id: entry.id.clone(),
                            group_id: entry.group_id.clone(),
                            stage: crate::models::ProgressStage::Finalizing,
                        });

                        if let Err(e) = run_ai_captions(&app, &dest_path, &config.ai, &uniqueness_config).await {
                            tracing::error!("AI Captions failed: {}", e);
                            return Err(YtdlpDownloadError::RunnerError(format!("AI Captions failed: {}", e)));
                        }
                    } else if uniqueness_config.enabled {
                        let _ = app.emit("media_progress_stage", MediaProgressStage {
                            id: entry.id.clone(),
                            group_id: entry.group_id.clone(),
                            stage: crate::models::ProgressStage::Finalizing,
                        });
                        if let Err(e) = run_basic_uniqueness(&app, &dest_path, &uniqueness_config).await {
                            tracing::error!("Basic Uniqueness failed: {}", e);
                            return Err(YtdlpDownloadError::RunnerError(format!("Basic Uniqueness failed: {}", e)));
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
                                return Err(YtdlpDownloadError::RunnerError(format!("Dubbing failed: {}", e)));
                            }
                         }
                    }

                    let mut generated_title = "Viral Short".to_string();
                    let mut generated_desc = "Auto-generated by ViralAutomator".to_string();

                    if uniqueness_config.ai_metadata && !config.ai.deepseek_api_key.is_empty() {
                        if let Ok((title, desc)) = crate::ai::deepseek::generate_metadata(&config.ai.deepseek_api_key, &entry.id).await {
                            tracing::info!("AI Metadata generated: Title={}, Desc={}", title, desc);
                            generated_title = title;
                            generated_desc = desc;
                        }
                    }

                    if uniqueness_config.auto_post_enabled {
                        tracing::info!("Auto-Posting to YouTube Shorts...");
                        let script_status = tokio::process::Command::new("node")
                            .arg("scripts/auto_poster/youtube_shorts.js")
                            .arg(&dest_path)
                            .arg(&generated_title)
                            .arg(&generated_desc)
                            .status()
                            .await;

                        match script_status {
                            Ok(s) if s.success() => tracing::info!("Auto-posting completed successfully!"),
                            Ok(s) => tracing::error!("Auto-posting script failed with status {}", s),
                            Err(e) => tracing::error!("Failed to launch auto-posting script: {}", e),
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

fn parse_progress_line(
  line: &str,
  progress_parser: &mut YtdlpProgressParser,
  app: &AppHandle,
) -> Option<ProgressEvent> {
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
  uniqueness: &crate::state::config_models::UniquenessSettings,
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
  let ffmpeg_exe = if cfg!(windows) {
    "ffmpeg.exe"
  } else {
    "ffmpeg"
  };
  let ffmpeg_path = bin_dir.join(ffmpeg_exe);
  let _ = TokioCommand::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args([
      "-i",
      dest_path,
      "-vn",
      "-ar",
      "16000",
      "-ac",
      "1",
      "-c:a",
      "libmp3lame",
      audio_path.to_str().ok_or("Invalid audio path")?,
      "-y",
    ])
    .status()
    .await;

  // 2. Transcribe
  let srt_content = if uniqueness.caption_provider == "groq" {
    crate::ai::groq::transcribe_audio(&ai.groq_api_key, &audio_path).await?
  } else {
    crate::ai::openai::transcribe_audio(&ai.openai_api_key, &audio_path).await?
  };
  tokio::fs::write(&srt_path, &srt_content)
    .await
    .map_err(|e| e.to_string())?;

  // 3. Identify segments using Deepseek
  let segments = crate::ai::shorts::identify_viral_segments(
    &ai.deepseek_api_key,
    &srt_content,
    uniqueness.num_shorts,
  )
  .await?;

  // 4. Extract and process each segment
  for (i, segment) in segments.iter().enumerate() {
    let output_name = format!(
      "{}_short_{}.mp4",
      video_path.file_stem().unwrap().to_str().unwrap(),
      i + 1
    );
    let output_path = video_path.parent().unwrap().join(output_name);

    // Create segment-specific SRT (simplified: just use the full SRT, FFmpeg subtitles filter handles timestamps)
    // For vertical crop: crop=ih*(9/16):ih
    let mut filters = vec!["crop=ih*(9/16):ih".to_string()];
    if uniqueness.flip_horizontal {
      filters.push("hflip".to_string());
    }

    let escaped_srt = srt_path
      .to_str()
      .unwrap()
      .replace("\\", "/")
      .replace(":", "\\:");
    filters.push(format!(
      "subtitles='{}':force_style='{}'",
      escaped_srt, ai.caption_style
    ));

    let filter_chain = filters.join(",");

    let args = vec![
      "-ss".to_string(),
      segment.start_time.clone(),
      "-to".to_string(),
      segment.end_time.clone(),
      "-i".to_string(),
      dest_path.to_string(),
      "-vf".to_string(),
      filter_chain,
      "-c:a".to_string(),
      "copy".to_string(),
      output_path.to_str().unwrap().to_string(),
      "-y".to_string(),
    ];

    let ffmpeg_exe = if cfg!(windows) {
      "ffmpeg.exe"
    } else {
      "ffmpeg"
    };
    let ffmpeg_path = bin_dir.join(ffmpeg_exe);
    let _ = TokioCommand::new(&ffmpeg_path)
      .env("PATH", &new_path)
      .args(&args)
      .status()
      .await;

    // Optional: If dubbing is enabled, dub each short
    if uniqueness.dubbing_enabled && !ai.deepseek_api_key.is_empty() {
      let _ =
        crate::ai::dubbing::run_ai_dubbing(app, &output_path, &srt_content, ai, uniqueness).await;
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
  uniqueness: &crate::state::config_models::UniquenessSettings,
) -> Result<String, String> {
  let paths_manager = app.state::<crate::paths::PathsManager>();
  let bin_dir = paths_manager.bin_dir();
  let separator = if cfg!(windows) { ';' } else { ':' };
  let path_env = std::env::var("PATH").unwrap_or_default();
  let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);

  let video_path = PathBuf::from(dest_path);
  let audio_path = video_path.with_extension("mp3_tmp");

  let ffmpeg_exe = if cfg!(windows) {
    "ffmpeg.exe"
  } else {
    "ffmpeg"
  };
  let ffmpeg_path = bin_dir.join(ffmpeg_exe);
  let _ = TokioCommand::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args([
      "-i",
      dest_path,
      "-vn",
      "-ar",
      "16000",
      "-ac",
      "1",
      "-c:a",
      "libmp3lame",
      audio_path.to_str().unwrap(),
      "-y",
    ])
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
  uniqueness: &crate::state::config_models::UniquenessSettings,
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
  let ffmpeg_exe = if cfg!(windows) {
    "ffmpeg.exe"
  } else {
    "ffmpeg"
  };
  let ffmpeg_path = bin_dir.join(ffmpeg_exe);
  let status = TokioCommand::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args([
      "-i",
      dest_path,
      "-vn",
      "-ar",
      "16000",
      "-ac",
      "1",
      "-c:a",
      "libmp3lame",
      audio_path.to_str().ok_or("Invalid audio path")?,
      "-y",
    ])
    .status()
    .await
    .map_err(|e| format!("FFmpeg extract failed: {}", e))?;

  if !status.success() {
    return Err("FFmpeg extract failed".to_string());
  }

  // 2. Transcribe
  let srt_content = if uniqueness.caption_provider == "groq" {
    let key = if !ai.groq_api_key.is_empty() {
      &ai.groq_api_key
    } else {
      &ai.deepseek_api_key // Fallback to Deepseek field if it contains a Groq key
    };
    crate::ai::groq::transcribe_audio(key, &audio_path).await?
  } else {
    crate::ai::openai::transcribe_audio(&ai.openai_api_key, &audio_path).await?
  };
  tokio::fs::write(&srt_path, srt_content)
    .await
    .map_err(|e| e.to_string())?;

  // 3. Burn captions
  let final_path = video_path.with_extension("final.mp4");
  let args = build_advanced_ffmpeg_args(
    dest_path,
    final_path.to_str().ok_or("Invalid final path")?,
    uniqueness,
    srt_path.to_str(),
    Some(&ai.caption_style),
  );

  let ffmpeg_exe = if cfg!(windows) {
    "ffmpeg.exe"
  } else {
    "ffmpeg"
  };
  let ffmpeg_path = bin_dir.join(ffmpeg_exe);
  let status = TokioCommand::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args(&args)
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

async fn run_basic_uniqueness(
  app: &AppHandle,
  dest_path: &str,
  uniqueness: &crate::state::config_models::UniquenessSettings,
) -> Result<(), String> {
  let video_path = std::path::PathBuf::from(dest_path);
  let final_path = video_path.with_extension("uniqueness.mp4");

  let paths_manager = app.state::<crate::paths::PathsManager>();
  let bin_dir = paths_manager.bin_dir();
  let separator = if cfg!(windows) { ';' } else { ':' };
  let path_env = std::env::var("PATH").unwrap_or_default();
  let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);

  let args = build_advanced_ffmpeg_args(
    dest_path,
    final_path.to_str().ok_or("Invalid final path")?,
    uniqueness,
    None,
    None,
  );

  let ffmpeg_exe = if cfg!(windows) {
    "ffmpeg.exe"
  } else {
    "ffmpeg"
  };
  let ffmpeg_path = bin_dir.join(ffmpeg_exe);
  let status = tokio::process::Command::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args(&args)
    .status()
    .await
    .map_err(|e| format!("FFmpeg uniqueness failed: {}", e))?;

  if status.success() {
    let _ = tokio::fs::rename(final_path, video_path).await;
  }
  Ok(())
}

fn build_advanced_ffmpeg_args(
  dest_path: &str,
  final_path: &str,
  uniqueness: &crate::state::config_models::UniquenessSettings,
  srt_path: Option<&str>,
  caption_style: Option<&str>,
) -> Vec<String> {
  let mut args = vec!["-i".to_string(), dest_path.to_string()];
  let mut filter_complex = Vec::new();
  let mut video_in = "0:v".to_string();
  let mut audio_in = "0:a".to_string();
  let mut current_input_idx = 1;
  let mut has_complex = false;
  let mut has_audio_filter = false;

  // Split Screen
  if uniqueness.split_screen_enabled && !uniqueness.split_screen_video_path.is_empty() {
    args.push("-i".to_string());
    args.push(uniqueness.split_screen_video_path.clone());

    filter_complex.push(format!("[{}]scale=1080:960:force_original_aspect_ratio=decrease,pad=1080:960:(ow-iw)/2:(oh-ih)/2[v_top]", video_in));
    filter_complex.push(format!(
      "[{}:v]scale=1080:960:force_original_aspect_ratio=increase,crop=1080:960[v_bottom]",
      current_input_idx
    ));
    filter_complex.push("[v_top][v_bottom]vstack=inputs=2[v_stacked]".to_string());
    video_in = "v_stacked".to_string();
    current_input_idx += 1;
    has_complex = true;
  } else if uniqueness.auto_crop {
    // Auto Crop
    filter_complex.push(format!("[{}]crop=ih*(9/16):ih[v_crop]", video_in));
    video_in = "v_crop".to_string();
    has_complex = true;
  }

  // HFlip
  if uniqueness.flip_horizontal {
    filter_complex.push(format!("[{}]hflip[v_flip]", video_in));
    video_in = "v_flip".to_string();
    has_complex = true;
  }

  // Micro Speed
  if uniqueness.micro_speed_enabled {
    filter_complex.push(format!("[{}]setpts=0.98*PTS[v_speed]", video_in));
    filter_complex.push(format!("[{}]atempo=1.02[a_speed]", audio_in));
    video_in = "v_speed".to_string();
    audio_in = "a_speed".to_string();
    has_complex = true;
    has_audio_filter = true;
  }

  // Subtitles
  if let (Some(srt), Some(style)) = (srt_path, caption_style) {
    let escaped_srt = srt.replace("\\", "/").replace(":", "\\:");
    filter_complex.push(format!(
      "[{}]subtitles='{}':force_style='{}'[v_sub]",
      video_in, escaped_srt, style
    ));
    video_in = "v_sub".to_string();
    has_complex = true;
  }

  // Watermark
  if !uniqueness.watermark_text.is_empty() {
    let escaped_text = uniqueness.watermark_text.replace('\'', "\\'");
    filter_complex.push(format!(
      "[{}]drawtext=text='{}':fontcolor=white:fontsize=24:x=(w-text_w)/2:y=h-th-20[v_water]",
      video_in, escaped_text
    ));
    video_in = "v_water".to_string();
    has_complex = true;
  }

  // Background Audio
  if uniqueness.background_audio_enabled && !uniqueness.background_audio_path.is_empty() {
    args.push("-i".to_string());
    args.push(uniqueness.background_audio_path.clone());
    filter_complex.push(format!(
      "[{}][{}:a]amix=inputs=2:duration=first:weights=9 1[a_mixed]",
      audio_in, current_input_idx
    ));
    audio_in = "a_mixed".to_string();
    has_complex = true;
    has_audio_filter = true;
  }

  if has_complex {
    args.push("-filter_complex".to_string());
    args.push(filter_complex.join(";"));
    args.push("-map".to_string());
    args.push(format!("[{}]", video_in));
    args.push("-map".to_string());
    if !has_audio_filter {
      args.push("0:a".to_string());
    } else {
      args.push(format!("[{}]", audio_in));
    }
  } else {
    args.push("-c:v".to_string());
    args.push("copy".to_string());
    args.push("-c:a".to_string());
    args.push("copy".to_string());
  }

  args.push(final_path.to_string());
  args.push("-y".to_string());
  args
}
