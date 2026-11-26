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
                  if let Some(mode) = u.crop_mode.as_ref() { uniqueness_config.crop_mode = mode.clone(); }
                  if let Some(watermark) = u.watermark_text.as_ref() { uniqueness_config.watermark_text = watermark.clone(); }
                  if let Some(ai_caps) = u.ai_captions { uniqueness_config.ai_captions = ai_caps; }
                  if let Some(hormozi) = u.hormozi_captions { uniqueness_config.hormozi_captions = hormozi; }
                  if let Some(hook_banner) = u.hook_banner_enabled { uniqueness_config.hook_banner_enabled = hook_banner; }
                  if let Some(ai_meta) = u.ai_metadata { uniqueness_config.ai_metadata = ai_meta; }
                  if let Some(provider) = u.caption_provider.as_ref() { uniqueness_config.caption_provider = provider.clone(); }
                  if let Some(ai_shorts) = u.ai_shorts_mode { uniqueness_config.ai_shorts_mode = ai_shorts; }
                  if let Some(num) = u.num_shorts { uniqueness_config.num_shorts = num; }
                  if let Some(dubbing) = u.dubbing_enabled { uniqueness_config.dubbing_enabled = dubbing; }
                  if let Some(target) = u.target_language.as_ref() { uniqueness_config.target_language = target.clone(); }
                  if let Some(voice) = u.voice_clone_id.as_ref() { uniqueness_config.voice_clone_id = voice.clone(); }
                  if let Some(split_scr) = u.split_screen_enabled { uniqueness_config.split_screen_enabled = split_scr; }
                  if let Some(split_path) = u.split_screen_video_path.as_ref() { uniqueness_config.split_screen_video_path = split_path.clone(); }
                  if let Some(speed) = u.micro_speed_enabled { uniqueness_config.micro_speed_enabled = speed; }
                  if let Some(hook) = u.ai_hook_rewriting { uniqueness_config.ai_hook_rewriting = hook; }
                  if let Some(bg_audio) = u.background_audio_enabled { uniqueness_config.background_audio_enabled = bg_audio; }
                  if let Some(bg_path) = u.background_audio_path.as_ref() { uniqueness_config.background_audio_path = bg_path.clone(); }
                  if let Some(auto_post) = u.auto_post_enabled { uniqueness_config.auto_post_enabled = auto_post; }
                  if let Some(auto_queue) = u.auto_queue_publish { uniqueness_config.auto_queue_publish = auto_queue; }
                  if let Some(platforms) = u.publish_platforms.as_ref() { uniqueness_config.publish_platforms = platforms.clone(); }
                  if let Some(drip) = u.drip_interval_hours { uniqueness_config.drip_interval_hours = drip; }
                }

                if uniqueness_config.enabled {
                    let is_audio_only = matches!(entry.format.track_type, crate::models::TrackType::Audio)
                        || dest_path.ends_with(".mp3")
                        || dest_path.ends_with(".m4a")
                        || dest_path.ends_with(".wav")
                        || dest_path.ends_with(".aac")
                        || dest_path.ends_with(".flac")
                        || dest_path.ends_with(".ogg")
                        || dest_path.ends_with(".opus");

                    if is_audio_only {
                        tracing::warn!("Skipping Faceless processing because the downloaded file is audio-only.");
                        let _ = app.emit("media_progress_stage", MediaProgressStage {
                            id: entry.id.clone(),
                            group_id: entry.group_id.clone(),
                            stage: crate::models::ProgressStage::Finalizing,
                        });
                    } else {
                        validate_uniqueness_files(&mut uniqueness_config);

                        let _ = app.emit("media_progress_stage", MediaProgressStage {
                            id: entry.id.clone(),
                            group_id: entry.group_id.clone(),
                            stage: crate::models::ProgressStage::Finalizing,
                        });

                        let input_title = entry
                            .template_context
                            .values
                            .get("title")
                            .filter(|s| !s.trim().is_empty())
                            .cloned()
                            .unwrap_or_else(|| {
                                Path::new(&dest_path)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Viral Video")
                                    .to_string()
                            });

                        let has_llm = crate::ai::has_any_llm_key(&config.ai);

                        if uniqueness_config.ai_shorts_mode && has_llm {
                            if let Err(e) = run_ai_shorts_extraction(
                                &app,
                                &dest_path,
                                &entry.url,
                                &input_title,
                                &config.ai,
                                &uniqueness_config,
                            ).await {
                                tracing::error!("AI Shorts extraction failed: {}", e);
                                return Err(YtdlpDownloadError::RunnerError(format!("AI Shorts extraction failed: {}", e)));
                            }
                        } else {
                            if uniqueness_config.ai_shorts_mode && !has_llm {
                                tracing::warn!("AI Shorts Mode enabled but no Gemini/DeepSeek/Groq API key configured. Skipping shorts extraction.");
                            }

                            let has_caption_key = crate::ai::has_valid_caption_key(&config.ai, &uniqueness_config.caption_provider);

                            // Generate transcript early if needed for captions, metadata, or dubbing
                            let mut transcript = String::new();
                            if (uniqueness_config.ai_captions || uniqueness_config.dubbing_enabled) && has_caption_key {
                                match run_ai_transcription(&app, &dest_path, &config.ai, &uniqueness_config).await {
                                    Ok(srt) => transcript = srt,
                                    Err(e) => {
                                        if uniqueness_config.ai_captions {
                                            tracing::error!("AI transcription failed: {}", e);
                                            return Err(YtdlpDownloadError::RunnerError(format!("AI transcription failed: {}", e)));
                                        } else {
                                            tracing::warn!("Transcription for dubbing failed: {}", e);
                                        }
                                    }
                                }
                            } else if uniqueness_config.ai_captions && !has_caption_key {
                                tracing::warn!("AI Captions enabled but API key missing. Falling back to basic uniqueness.");
                            }

                            // Generate multi-platform metadata (and 3s viral hook text!) before rendering
                            let metadata = if (uniqueness_config.ai_metadata || uniqueness_config.hook_banner_enabled) && has_llm {
                                crate::ai::generate_metadata_auto(&config.ai, &input_title, &transcript)
                                    .await
                                    .unwrap_or_else(|e| {
                                        tracing::warn!("AI metadata generation fell back to defaults: {}", e);
                                        crate::ai::MultiPlatformMetadata::fallback(&input_title)
                                    })
                            } else {
                                crate::ai::MultiPlatformMetadata::fallback(&input_title)
                            };

                            let hook_opt = if uniqueness_config.hook_banner_enabled && !metadata.hook_text.trim().is_empty() {
                                Some(metadata.hook_text.as_str())
                            } else {
                                None
                            };

                            if (uniqueness_config.ai_captions && !transcript.is_empty()) || hook_opt.is_some() {
                                if let Err(e) = run_ai_captions_and_filters(
                                    &app,
                                    &dest_path,
                                    &transcript,
                                    hook_opt,
                                    &config.ai,
                                    &uniqueness_config,
                                ).await {
                                    tracing::error!("AI Captions/Uniqueness failed: {}", e);
                                    return Err(YtdlpDownloadError::RunnerError(format!("AI Captions failed: {}", e)));
                                }
                            } else if let Err(e) = run_basic_uniqueness(&app, &dest_path, &uniqueness_config).await {
                                tracing::error!("Basic Uniqueness failed: {}", e);
                                return Err(YtdlpDownloadError::RunnerError(format!("Basic Uniqueness failed: {}", e)));
                            }

                            if uniqueness_config.dubbing_enabled && has_llm && !transcript.is_empty() {
                                if let Err(e) = crate::ai::dubbing::run_ai_dubbing(&app, Path::new(&dest_path), &transcript, &config.ai, &uniqueness_config).await {
                                    tracing::error!("Dubbing failed: {}", e);
                                    return Err(YtdlpDownloadError::RunnerError(format!("Dubbing failed: {}", e)));
                                }
                            }

                            if uniqueness_config.auto_queue_publish || uniqueness_config.auto_post_enabled {
                                enqueue_processed_clip(
                                    &app,
                                    &dest_path,
                                    Some(entry.url.clone()),
                                    &input_title,
                                    None,
                                    None,
                                    metadata,
                                    &uniqueness_config,
                                ).await;
                            }
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

fn ffmpeg_bin_and_path(app: &AppHandle) -> (PathBuf, String) {
  let paths_manager = app.state::<crate::paths::PathsManager>();
  let bin_dir = paths_manager.bin_dir();
  let separator = if cfg!(windows) { ';' } else { ':' };
  let path_env = std::env::var("PATH").unwrap_or_default();
  let new_path = format!("{}{}{}", bin_dir.display(), separator, path_env);
  let ffmpeg_exe = if cfg!(windows) {
    "ffmpeg.exe"
  } else {
    "ffmpeg"
  };
  (bin_dir.join(ffmpeg_exe), new_path)
}

async fn extract_video_thumbnail(app: &AppHandle, video_path: &Path) -> Option<String> {
  let (ffmpeg_path, new_path) = ffmpeg_bin_and_path(app);
  let thumb_path = video_path.with_extension("thumb.jpg");
  let video_str = video_path.to_str()?;
  let thumb_str = thumb_path.to_str()?;

  let status = TokioCommand::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args([
      "-ss",
      "00:00:01.5",
      "-i",
      video_str,
      "-vframes",
      "1",
      "-q:v",
      "3",
      thumb_str,
      "-y",
    ])
    .status()
    .await
    .ok()?;

  if status.success() && thumb_path.exists() {
    Some(thumb_str.to_string())
  } else {
    None
  }
}

#[allow(clippy::too_many_arguments)]
async fn enqueue_processed_clip(
  app: &AppHandle,
  video_path: &str,
  source_url: Option<String>,
  source_title: &str,
  virality_score: Option<u8>,
  reason: Option<String>,
  metadata: crate::ai::MultiPlatformMetadata,
  uniqueness: &crate::state::config_models::UniquenessSettings,
) {
  let Some(queue) = app.try_state::<crate::publish::queue::PublishQueueState>() else {
    return;
  };

  let thumbnail_path = extract_video_thumbnail(app, Path::new(video_path)).await;
  let platforms = &uniqueness.publish_platforms;
  let yt_enabled = platforms.iter().any(|p| p == "youtube");
  let tt_enabled = platforms.iter().any(|p| p == "tiktok");
  let ig_enabled = platforms.iter().any(|p| p == "instagram");

  let scheduled_at = if uniqueness.auto_post_enabled {
    Some(queue.next_drip_slot(uniqueness.drip_interval_hours))
  } else {
    None
  };

  let make_state = |enabled: bool| {
    let mut state = crate::publish::models::PlatformPostState::new(enabled);
    if enabled && scheduled_at.is_some() {
      state.status = crate::publish::models::PublishStatus::Scheduled;
    }
    state
  };

  let item = crate::publish::models::PublishItem {
    id: uuid::Uuid::new_v4().to_string(),
    video_path: video_path.to_string(),
    thumbnail_path,
    source_title: source_title.to_string(),
    source_url,
    virality_score,
    hook_text: metadata.hook_text,
    reason,
    youtube_title: metadata.youtube_title,
    youtube_description: metadata.youtube_description,
    tiktok_caption: metadata.tiktok_caption,
    instagram_caption: metadata.instagram_caption,
    hashtags: metadata.hashtags,
    scheduled_at,
    created_at: crate::publish::queue::now_epoch_secs(),
    youtube: make_state(yt_enabled),
    tiktok: make_state(tt_enabled),
    instagram: make_state(ig_enabled),
  };

  queue.add(app, item);
  tracing::info!("Enqueued clip into Publish Studio: {}", video_path);
}

async fn transcribe_with_provider(
  ai: &AiSettings,
  provider: &str,
  audio_path: &Path,
) -> Result<String, String> {
  match provider {
    "groq" => {
      let key = if !ai.groq_api_key.is_empty() {
        &ai.groq_api_key
      } else {
        &ai.deepseek_api_key
      };
      crate::ai::groq::transcribe_audio(key, audio_path).await
    }
    "openai" if !ai.openai_api_key.trim().is_empty() => {
      crate::ai::openai::transcribe_audio(&ai.openai_api_key, audio_path).await
    }
    _ => crate::ai::gemini::transcribe_audio(&ai.gemini_api_key, audio_path).await,
  }
}

fn parse_clock_secs(value: &str) -> Option<f64> {
  let parts = value.trim().split(':').collect::<Vec<_>>();
  if parts.len() == 3 {
    let h = parts[0].trim().parse::<f64>().ok()?;
    let m = parts[1].trim().parse::<f64>().ok()?;
    let s = parts[2].trim().parse::<f64>().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
  } else if parts.len() == 2 {
    let m = parts[0].trim().parse::<f64>().ok()?;
    let s = parts[1].trim().parse::<f64>().ok()?;
    Some(m * 60.0 + s)
  } else if parts.len() == 1 {
    parts[0].trim().parse::<f64>().ok()
  } else {
    None
  }
}

fn parse_srt_timestamp(ts: &str) -> Option<f64> {
  let ts = ts.trim().replace(',', ".");
  let parts: Vec<&str> = ts.split(':').collect();
  if parts.len() == 3 {
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let s: f64 = parts[2].parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
  } else if parts.len() == 2 {
    let m: f64 = parts[0].parse().ok()?;
    let s: f64 = parts[1].parse().ok()?;
    Some(m * 60.0 + s)
  } else {
    parts[0].parse().ok()
  }
}

fn format_srt_timestamp(secs: f64) -> String {
  let total_millis = (secs * 1000.0).round() as u64;
  let millis = total_millis % 1000;
  let total_secs = total_millis / 1000;
  let s = total_secs % 60;
  let total_mins = total_secs / 60;
  let m = total_mins % 60;
  let h = total_mins / 60;
  format!("{:02}:{:02}:{:02},{:03}", h, m, s, millis)
}

fn slice_and_shift_srt(full_srt: &str, start_secs: f64, end_secs: f64) -> String {
  let mut result = String::new();
  let mut index = 1;

  let normalized = full_srt.replace("\r\n", "\n");
  let blocks = normalized.split("\n\n");

  for block in blocks {
    let lines: Vec<&str> = block
      .lines()
      .map(|l| l.trim())
      .filter(|l| !l.is_empty())
      .collect();
    if lines.len() < 2 {
      continue;
    }

    if let Some(time_line) = lines.iter().find(|l| l.contains(" --> ")) {
      let parts: Vec<&str> = time_line.split(" --> ").collect();
      if parts.len() == 2 {
        if let (Some(sub_start), Some(sub_end)) =
          (parse_srt_timestamp(parts[0]), parse_srt_timestamp(parts[1]))
        {
          if sub_end > start_secs && sub_start < end_secs {
            let shifted_start = (sub_start - start_secs).max(0.0);
            let shifted_end = (sub_end - start_secs)
              .min(end_secs - start_secs)
              .max(shifted_start + 0.1);

            let text_lines: Vec<&str> = lines
              .iter()
              .skip_while(|l| !l.contains(" --> "))
              .skip(1)
              .copied()
              .collect();
            let text = text_lines.join("\n");

            if !text.is_empty() {
              result.push_str(&format!("{}\n", index));
              result.push_str(&format!(
                "{} --> {}\n",
                format_srt_timestamp(shifted_start),
                format_srt_timestamp(shifted_end)
              ));
              result.push_str(&format!("{}\n\n", text));
              index += 1;
            }
          }
        }
      }
    }
  }

  result
}

fn validate_uniqueness_files(uniqueness: &mut crate::state::config_models::UniquenessSettings) {
  if uniqueness.background_audio_enabled && !uniqueness.background_audio_path.is_empty() {
    let path = Path::new(&uniqueness.background_audio_path);
    if !path.exists() {
      tracing::warn!(
        "Background audio file not found: {}, disabling background audio.",
        uniqueness.background_audio_path
      );
      uniqueness.background_audio_enabled = false;
    }
  }
  if uniqueness.split_screen_enabled && !uniqueness.split_screen_video_path.is_empty() {
    let path = Path::new(&uniqueness.split_screen_video_path);
    if !path.exists() {
      tracing::warn!(
        "Split-screen gameplay video file not found: {}, disabling split-screen.",
        uniqueness.split_screen_video_path
      );
      uniqueness.split_screen_enabled = false;
    }
  }
}

async fn run_ai_shorts_extraction(
  app: &AppHandle,
  dest_path: &str,
  source_url: &str,
  source_title: &str,
  ai: &AiSettings,
  uniqueness: &crate::state::config_models::UniquenessSettings,
) -> Result<(), String> {
  let (ffmpeg_path, new_path) = ffmpeg_bin_and_path(app);
  let video_path = PathBuf::from(dest_path);
  let audio_path = video_path.with_extension("mp3");
  let srt_path = video_path.with_extension("srt");

  // 1. Extract audio for transcription
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
    .map_err(|e| format!("FFmpeg audio extraction failed: {}", e))?;

  if !status.success() {
    return Err("FFmpeg failed to extract audio from video for shorts transcription".to_string());
  }

  // 2. Transcribe (Gemini-first default)
  let srt_content = transcribe_with_provider(ai, &uniqueness.caption_provider, &audio_path).await?;
  tokio::fs::write(&srt_path, &srt_content)
    .await
    .map_err(|e| e.to_string())?;

  // 3. Identify highest-retention viral segments (Gemini-first with DeepSeek/Groq fallback)
  let num_clips = uniqueness.num_shorts.clamp(1, 10);
  let segments = crate::ai::identify_viral_segments_auto(ai, &srt_content, num_clips).await?;

  let parent_dir = video_path.parent().unwrap_or_else(|| Path::new("."));
  let base_stem = video_path
    .file_stem()
    .and_then(|s| s.to_str())
    .unwrap_or("video");

  // 4. Extract, style (9:16 + Hormozi captions + 3s hook banner), and queue each viral short
  for (i, segment) in segments.iter().enumerate() {
    let output_name = format!("{}_short_{}.mp4", base_stem, i + 1);
    let output_path = parent_dir.join(output_name);

    let seg_start = parse_srt_timestamp(&segment.start_time).unwrap_or(0.0);
    let seg_end = parse_srt_timestamp(&segment.end_time).unwrap_or(seg_start + 45.0);
    let seg_srt_content = slice_and_shift_srt(&srt_content, seg_start, seg_end);
    let has_seg_srt = !seg_srt_content.trim().is_empty();

    let clip_title = if !segment.title.trim().is_empty() {
      segment.title.clone()
    } else {
      format!("{} Part {}", source_title, i + 1)
    };

    // Generate platform-tailored YT Shorts + TikTok + IG Reels metadata & hook
    let mut metadata = if uniqueness.ai_metadata && crate::ai::has_any_llm_key(ai) {
      crate::ai::generate_metadata_auto(ai, &clip_title, &seg_srt_content)
        .await
        .unwrap_or_else(|_| crate::ai::MultiPlatformMetadata::fallback(&clip_title))
    } else {
      crate::ai::MultiPlatformMetadata::fallback(&clip_title)
    };

    if let Some(seg_hook) = segment
      .hook_text
      .as_deref()
      .filter(|h| !h.trim().is_empty())
    {
      metadata.hook_text = seg_hook.trim().to_uppercase();
    }

    // Build subtitle overlay file (Hormozi animated ASS + 3s hook banner OR classic SRT)
    let use_ass = uniqueness.hormozi_captions || uniqueness.hook_banner_enabled;
    let sub_path = if use_ass {
      parent_dir.join(format!("{}_short_{}.ass", base_stem, i + 1))
    } else {
      parent_dir.join(format!("{}_short_{}.srt", base_stem, i + 1))
    };

    let hook_opt = if uniqueness.hook_banner_enabled && !metadata.hook_text.trim().is_empty() {
      Some(metadata.hook_text.as_str())
    } else {
      None
    };

    let wrote_sub = if use_ass && (has_seg_srt || hook_opt.is_some()) {
      let ass_doc = crate::ai::captions::build_hormozi_ass(
        &seg_srt_content,
        hook_opt,
        uniqueness.ai_captions && has_seg_srt,
      );
      tokio::fs::write(&sub_path, ass_doc).await.is_ok()
    } else if has_seg_srt && uniqueness.ai_captions {
      tokio::fs::write(&sub_path, &seg_srt_content).await.is_ok()
    } else {
      false
    };

    // Always ensure 9:16 vertical framing for extracted shorts
    let mut short_uniqueness = uniqueness.clone();
    if !short_uniqueness.split_screen_enabled {
      short_uniqueness.auto_crop = true;
    }

    let args = build_advanced_ffmpeg_args(
      dest_path,
      output_path.to_str().ok_or("Invalid short output path")?,
      &short_uniqueness,
      if wrote_sub { sub_path.to_str() } else { None },
      use_ass,
      Some(&ai.caption_style),
      Some((&segment.start_time, &segment.end_time)),
    );

    let status = TokioCommand::new(&ffmpeg_path)
      .env("PATH", &new_path)
      .args(&args)
      .status()
      .await
      .map_err(|e| format!("FFmpeg short extraction failed: {}", e))?;

    if wrote_sub {
      let _ = tokio::fs::remove_file(&sub_path).await;
    }

    if !status.success() {
      return Err(format!(
        "FFmpeg failed during short extraction for segment: {} - {}",
        segment.start_time, segment.end_time
      ));
    }

    // Optional: Dub each short with its segment transcript
    if uniqueness.dubbing_enabled && crate::ai::has_any_llm_key(ai) {
      let dub_transcript = if has_seg_srt {
        &seg_srt_content
      } else {
        &srt_content
      };
      let _ =
        crate::ai::dubbing::run_ai_dubbing(app, &output_path, dub_transcript, ai, uniqueness).await;
    }

    // Queue clip into the multi-platform Publish Studio (with optional staggered drip schedule)
    if uniqueness.auto_queue_publish || uniqueness.auto_post_enabled {
      enqueue_processed_clip(
        app,
        output_path.to_str().unwrap_or_default(),
        Some(source_url.to_string()),
        &clip_title,
        Some(segment.virality_score),
        segment.reason.clone(),
        metadata,
        uniqueness,
      )
      .await;
    }
  }

  // Cleanup temp files
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
  let (ffmpeg_path, new_path) = ffmpeg_bin_and_path(app);
  let video_path = PathBuf::from(dest_path);
  let audio_path = video_path.with_extension("mp3_tmp");

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
    .map_err(|e| format!("FFmpeg audio extraction failed: {}", e))?;

  if !status.success() {
    return Err("FFmpeg failed to extract audio from video for transcription".to_string());
  }

  let srt_content = transcribe_with_provider(ai, &uniqueness.caption_provider, &audio_path).await;
  let _ = tokio::fs::remove_file(audio_path).await;
  srt_content
}

async fn run_ai_captions_and_filters(
  app: &AppHandle,
  dest_path: &str,
  srt_content: &str,
  hook_banner: Option<&str>,
  ai: &AiSettings,
  uniqueness: &crate::state::config_models::UniquenessSettings,
) -> Result<(), String> {
  let (ffmpeg_path, new_path) = ffmpeg_bin_and_path(app);
  let video_path = PathBuf::from(dest_path);
  let use_ass = uniqueness.hormozi_captions || hook_banner.is_some();
  let sub_path = if use_ass {
    video_path.with_extension("hormozi.ass")
  } else {
    video_path.with_extension("srt")
  };

  if use_ass {
    let ass_doc = crate::ai::captions::build_hormozi_ass(
      srt_content,
      hook_banner,
      uniqueness.ai_captions && !srt_content.trim().is_empty(),
    );
    tokio::fs::write(&sub_path, ass_doc)
      .await
      .map_err(|e| e.to_string())?;
  } else {
    tokio::fs::write(&sub_path, srt_content)
      .await
      .map_err(|e| e.to_string())?;
  }

  let final_path = video_path.with_extension("final.mp4");
  let args = build_advanced_ffmpeg_args(
    dest_path,
    final_path.to_str().ok_or("Invalid final path")?,
    uniqueness,
    sub_path.to_str(),
    use_ass,
    Some(&ai.caption_style),
    None,
  );

  let status = TokioCommand::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args(&args)
    .status()
    .await
    .map_err(|e| format!("FFmpeg burn failed: {}", e))?;

  let _ = tokio::fs::remove_file(&sub_path).await;

  if status.success() {
    let _ = tokio::fs::rename(final_path, video_path).await;
    Ok(())
  } else {
    Err("FFmpeg failed during caption burning. Check the logs for details.".to_string())
  }
}

async fn run_basic_uniqueness(
  app: &AppHandle,
  dest_path: &str,
  uniqueness: &crate::state::config_models::UniquenessSettings,
) -> Result<(), String> {
  let video_path = std::path::PathBuf::from(dest_path);
  let final_path = video_path.with_extension("uniqueness.mp4");
  let (ffmpeg_path, new_path) = ffmpeg_bin_and_path(app);

  let args = build_advanced_ffmpeg_args(
    dest_path,
    final_path.to_str().ok_or("Invalid final path")?,
    uniqueness,
    None,
    false,
    None,
    None,
  );

  let status = tokio::process::Command::new(&ffmpeg_path)
    .env("PATH", &new_path)
    .args(&args)
    .status()
    .await
    .map_err(|e| format!("FFmpeg uniqueness failed: {}", e))?;

  if status.success() {
    let _ = tokio::fs::rename(final_path, video_path).await;
    Ok(())
  } else {
    Err("FFmpeg failed during basic uniqueness processing. Check the logs for details.".to_string())
  }
}

fn build_advanced_ffmpeg_args(
  dest_path: &str,
  final_path: &str,
  uniqueness: &crate::state::config_models::UniquenessSettings,
  sub_path: Option<&str>,
  is_ass: bool,
  caption_style: Option<&str>,
  segment_trim: Option<(&str, &str)>,
) -> Vec<String> {
  let mut args = Vec::new();
  if let Some((start, end)) = segment_trim {
    args.push("-ss".to_string());
    args.push(start.to_string());
    args.push("-to".to_string());
    args.push(end.to_string());
  }
  args.push("-i".to_string());
  args.push(dest_path.to_string());

  let mut filter_complex = Vec::new();
  let mut video_in = "0:v".to_string();
  let mut audio_in = "0:a".to_string();
  let mut current_input_idx = 1;
  let mut has_complex = false;
  let mut has_audio_filter = false;

  // Split Screen (loops short gameplay infinitely and terminates when main clip ends!)
  if uniqueness.split_screen_enabled && !uniqueness.split_screen_video_path.is_empty() {
    args.push("-stream_loop".to_string());
    args.push("-1".to_string());
    args.push("-i".to_string());
    args.push(uniqueness.split_screen_video_path.clone());

    filter_complex.push(format!(
      "[{}]scale=1080:960:force_original_aspect_ratio=decrease,pad=1080:960:(ow-iw)/2:(oh-ih)/2,setsar=1[v_top]",
      video_in
    ));
    filter_complex.push(format!(
      "[{}:v]scale=1080:960:force_original_aspect_ratio=increase,crop=1080:960,setsar=1[v_bottom]",
      current_input_idx
    ));
    filter_complex.push("[v_top][v_bottom]vstack=inputs=2:shortest=1[v_stacked]".to_string());
    video_in = "v_stacked".to_string();
    current_input_idx += 1;
    has_complex = true;
  } else if uniqueness.auto_crop {
    if uniqueness.crop_mode == "blur_pad" {
      // Blurred 1080x1920 background + full uncropped center foreground (ideal for multi-speaker podcasts)
      filter_complex.push(format!("[{}]split=2[v_bg_src][v_fg_src]", video_in));
      filter_complex.push(
        "[v_bg_src]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,boxblur=25:4,setsar=1[v_bg]"
          .to_string(),
      );
      filter_complex.push(
        "[v_fg_src]scale=1080:1920:force_original_aspect_ratio=decrease,setsar=1[v_fg]".to_string(),
      );
      filter_complex.push("[v_bg][v_fg]overlay=(W-w)/2:(H-h)/2[v_crop]".to_string());
    } else {
      // Safe scale-to-fill 9:16 (works on both 16:9 landscape and already-vertical videos!)
      filter_complex.push(format!(
        "[{}]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,setsar=1[v_crop]",
        video_in
      ));
    }
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
    filter_complex.push(format!("[{}]setpts=0.95*PTS[v_speed]", video_in));
    filter_complex.push(format!("[{}]atempo=1.05[a_speed]", audio_in));
    video_in = "v_speed".to_string();
    audio_in = "a_speed".to_string();
    has_complex = true;
    has_audio_filter = true;
  }

  // Subtitles (Hormozi ASS / Hook Banner OR classic SRT)
  if let Some(path) = sub_path {
    let escaped = path
      .replace('\\', "/")
      .replace(':', "\\:")
      .replace('\'', "'\\\\\\''");
    if is_ass {
      filter_complex.push(format!("[{}]ass='{}'[v_sub]", video_in, escaped));
    } else if let Some(style) = caption_style {
      filter_complex.push(format!(
        "[{}]subtitles='{}':force_style='{}'[v_sub]",
        video_in, escaped, style
      ));
    } else {
      filter_complex.push(format!("[{}]subtitles='{}'[v_sub]", video_in, escaped));
    }
    video_in = "v_sub".to_string();
    has_complex = true;
  }

  // Watermark
  if !uniqueness.watermark_text.is_empty() {
    let escaped_text = uniqueness.watermark_text.replace('\'', "\\'");
    filter_complex.push(format!(
      "[{}]drawtext=text='{}':fontcolor=white@0.85:fontsize=34:x=(w-text_w)/2:y=h-th-110[v_water]",
      video_in, escaped_text
    ));
    video_in = "v_water".to_string();
    has_complex = true;
  }

  // Background Audio (loops seamlessly if shorter than main video)
  if uniqueness.background_audio_enabled && !uniqueness.background_audio_path.is_empty() {
    args.push("-stream_loop".to_string());
    args.push("-1".to_string());
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
      args.push("0:a?".to_string());
      if segment_trim.is_some() {
        args.push("-c:a".to_string());
        args.push("aac".to_string());
        args.push("-b:a".to_string());
        args.push("192k".to_string());
      } else {
        args.push("-c:a".to_string());
        args.push("copy".to_string());
      }
    } else {
      args.push(format!("[{}]", audio_in));
      args.push("-c:a".to_string());
      args.push("aac".to_string());
      args.push("-b:a".to_string());
      args.push("192k".to_string());
    }
    args.push("-c:v".to_string());
    args.push("libx264".to_string());
    args.push("-preset".to_string());
    args.push("veryfast".to_string());
    args.push("-crf".to_string());
    args.push("20".to_string());
    args.push("-pix_fmt".to_string());
    args.push("yuv420p".to_string());
    args.push("-movflags".to_string());
    args.push("+faststart".to_string());
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::state::config_models::UniquenessSettings;

  #[test]
  fn test_parse_clock_secs() {
    assert_eq!(
      parse_clock_secs("01:23:45"),
      Some(3600.0 + 23.0 * 60.0 + 45.0)
    );
    assert_eq!(parse_clock_secs("05:30"), Some(330.0));
    assert_eq!(parse_clock_secs("45"), Some(45.0));
    assert_eq!(parse_clock_secs("invalid"), None);
  }

  #[test]
  fn test_parse_and_format_srt_timestamp() {
    let ts = "00:01:23,456";
    let secs = parse_srt_timestamp(ts).unwrap();
    assert!((secs - 83.456).abs() < 1e-4);
    assert_eq!(format_srt_timestamp(secs), ts);
  }

  #[test]
  fn test_slice_and_shift_srt() {
    let srt = "1\n00:00:10,000 --> 00:00:20,000\nHello World\n\n2\n00:00:25,000 --> 00:00:35,000\nSecond Line\n";
    let shifted = slice_and_shift_srt(srt, 15.0, 30.0);
    assert!(shifted.contains("00:00:00,000 --> 00:00:05,000"));
    assert!(shifted.contains("Hello World"));
    assert!(shifted.contains("00:00:10,000 --> 00:00:15,000"));
    assert!(shifted.contains("Second Line"));
  }

  #[test]
  fn test_build_advanced_ffmpeg_args_blur_pad_and_ass() {
    let cfg = UniquenessSettings {
      enabled: true,
      auto_crop: true,
      crop_mode: "blur_pad".into(),
      ..Default::default()
    };
    let args = build_advanced_ffmpeg_args(
      "in.mp4",
      "out.mp4",
      &cfg,
      Some("/tmp/captions.ass"),
      true,
      None,
      Some(("00:00:10", "00:00:45")),
    );
    let joined = args.join(" ");
    assert!(joined.contains("-ss 00:00:10 -to 00:00:45"));
    assert!(joined.contains("boxblur=25:4"));
    assert!(joined.contains("ass='/tmp/captions.ass'"));
    assert!(joined.contains("-movflags +faststart"));
  }

  #[test]
  fn test_build_advanced_ffmpeg_args_split_screen_loops_gameplay() {
    let cfg = UniquenessSettings {
      enabled: true,
      split_screen_enabled: true,
      split_screen_video_path: "/tmp/gameplay.mp4".into(),
      ..Default::default()
    };
    let args = build_advanced_ffmpeg_args("in.mp4", "out.mp4", &cfg, None, false, None, None);
    let joined = args.join(" ");
    assert!(joined.contains("-stream_loop -1 -i /tmp/gameplay.mp4"));
    assert!(joined.contains("vstack=inputs=2:shortest=1"));
  }
}
