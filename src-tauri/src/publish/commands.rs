use crate::ai::MultiPlatformMetadata;
use crate::publish::models::{
  PlatformAuthStatus, PlatformKind, PlatformPostState, PublishItem, PublishItemUpdate,
  PublishStatus,
};
use crate::publish::queue::{now_epoch_secs, PublishQueueState};
use crate::publish::runner::{check_auth_status, execute_platform_publish, open_platform_login};
use crate::SharedConfig;
use std::path::Path;
use tauri::{AppHandle, State};
use uuid::Uuid;

#[tauri::command]
pub fn publish_queue_list(queue: State<'_, PublishQueueState>) -> Vec<PublishItem> {
  queue.list()
}

#[tauri::command]
pub async fn publish_queue_add(
  app: AppHandle,
  queue: State<'_, PublishQueueState>,
  config: State<'_, SharedConfig>,
  video_path: String,
  source_title: Option<String>,
  generate_ai_metadata: Option<bool>,
) -> Result<PublishItem, String> {
  let path = Path::new(&video_path);
  if !path.exists() {
    return Err(format!("Video file does not exist: {video_path}"));
  }

  let title = source_title
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| {
      path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Viral Short")
        .to_string()
    });

  let cfg = config.load();
  let meta = if generate_ai_metadata.unwrap_or(true) && crate::ai::has_any_llm_key(&cfg.ai) {
    crate::ai::generate_metadata_auto(&cfg.ai, &title, "")
      .await
      .unwrap_or_else(|_| MultiPlatformMetadata::fallback(&title))
  } else {
    MultiPlatformMetadata::fallback(&title)
  };

  let platforms = &cfg.output.video.uniqueness.publish_platforms;
  let item = PublishItem {
    id: Uuid::new_v4().to_string(),
    video_path,
    thumbnail_path: None,
    source_title: title,
    source_url: None,
    virality_score: None,
    hook_text: meta.hook_text,
    reason: None,
    youtube_title: meta.youtube_title,
    youtube_description: meta.youtube_description,
    tiktok_caption: meta.tiktok_caption,
    instagram_caption: meta.instagram_caption,
    hashtags: meta.hashtags,
    scheduled_at: None,
    created_at: now_epoch_secs(),
    youtube: PlatformPostState::new(platforms.iter().any(|p| p == "youtube")),
    tiktok: PlatformPostState::new(platforms.iter().any(|p| p == "tiktok")),
    instagram: PlatformPostState::new(platforms.iter().any(|p| p == "instagram")),
  };

  Ok(queue.add(&app, item))
}

#[tauri::command]
pub fn publish_queue_update(
  app: AppHandle,
  queue: State<'_, PublishQueueState>,
  id: String,
  patch: PublishItemUpdate,
) -> Result<PublishItem, String> {
  queue.update(&app, &id, patch)
}

#[tauri::command]
pub fn publish_queue_remove(app: AppHandle, queue: State<'_, PublishQueueState>, id: String) {
  queue.remove(&app, &id);
}

#[tauri::command]
pub fn publish_queue_clear_posted(app: AppHandle, queue: State<'_, PublishQueueState>) -> usize {
  queue.clear_posted(&app)
}

#[tauri::command]
pub fn publish_auto_schedule(
  app: AppHandle,
  queue: State<'_, PublishQueueState>,
  interval_hours: u32,
) -> Vec<PublishItem> {
  queue.auto_schedule_pending(&app, interval_hours)
}

#[tauri::command]
pub async fn publish_item_now(
  app: AppHandle,
  queue: State<'_, PublishQueueState>,
  id: String,
  platform: Option<PlatformKind>,
) -> Result<(), String> {
  let item = queue
    .get(&id)
    .ok_or_else(|| format!("Publish queue item not found: {id}"))?;

  let targets: Vec<PlatformKind> = match platform {
    Some(p) => vec![p],
    None => PlatformKind::all()
      .into_iter()
      .filter(|&p| {
        let s = item.platform_state(p);
        s.enabled && s.status != PublishStatus::Posted
      })
      .collect(),
  };

  if targets.is_empty() {
    return Err("No enabled unposted platforms selected for this item".to_string());
  }

  let mut errors = Vec::new();
  for target in targets {
    let latest = queue.get(&id).unwrap_or_else(|| item.clone());
    if let Err(e) = execute_platform_publish(&app, &latest, target).await {
      errors.push(format!("{}: {}", target.profile_name(), e));
    }
  }

  if errors.is_empty() {
    Ok(())
  } else {
    Err(errors.join(" | "))
  }
}

#[tauri::command]
pub async fn publish_open_login(platform: PlatformKind) -> Result<(), String> {
  open_platform_login(platform).await
}

#[tauri::command]
pub fn publish_auth_status() -> PlatformAuthStatus {
  check_auth_status()
}

#[tauri::command]
pub async fn publish_regenerate_metadata(
  app: AppHandle,
  queue: State<'_, PublishQueueState>,
  config: State<'_, SharedConfig>,
  id: String,
) -> Result<PublishItem, String> {
  let item = queue
    .get(&id)
    .ok_or_else(|| format!("Publish queue item not found: {id}"))?;
  let cfg = config.load();
  let meta =
    crate::ai::generate_metadata_auto(&cfg.ai, &item.source_title, &item.hook_text).await?;

  queue.update(
    &app,
    &id,
    PublishItemUpdate {
      hook_text: Some(meta.hook_text),
      youtube_title: Some(meta.youtube_title),
      youtube_description: Some(meta.youtube_description),
      tiktok_caption: Some(meta.tiktok_caption),
      instagram_caption: Some(meta.instagram_caption),
      hashtags: Some(meta.hashtags),
      ..Default::default()
    },
  )
}
