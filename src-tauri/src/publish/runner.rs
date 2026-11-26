use crate::publish::models::{PlatformAuthStatus, PlatformKind, PublishItem, PublishStatus};
use crate::publish::queue::{now_epoch_secs, PublishQueueState};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::process::Command as TokioCommand;
use tokio::sync::Mutex as TokioMutex;

/// Ensures Playwright uploads run sequentially so persistent browser profiles never lock-collide.
#[derive(Clone)]
pub struct PublishExecutionLock(pub Arc<TokioMutex<()>>);

impl Default for PublishExecutionLock {
  fn default() -> Self {
    Self(Arc::new(TokioMutex::new(())))
  }
}

pub fn resolve_auto_poster_dir() -> PathBuf {
  let candidates = [
    PathBuf::from("scripts/auto_poster"),
    PathBuf::from("../scripts/auto_poster"),
  ];
  for candidate in candidates {
    if candidate.exists() {
      return candidate;
    }
  }
  if let Ok(exe) = std::env::current_exe() {
    if let Some(parent) = exe.parent() {
      let bundled = parent.join("scripts/auto_poster");
      if bundled.exists() {
        return bundled;
      }
    }
  }
  PathBuf::from("scripts/auto_poster")
}

pub fn resolve_auth_profiles_root() -> PathBuf {
  let dir = resolve_auto_poster_dir();
  dir
    .parent()
    .and_then(|p| p.parent())
    .unwrap_or_else(|| Path::new("."))
    .join(".auth_profiles")
}

fn enhanced_node_path() -> String {
  let path_env = std::env::var("PATH").unwrap_or_default();
  if cfg!(target_os = "macos") {
    format!("/opt/homebrew/bin:/usr/local/bin:{path_env}")
  } else {
    path_env
  }
}

fn profile_has_session(profile_dir: &Path) -> bool {
  if !profile_dir.exists() {
    return false;
  }
  // Chromium persistent context creates Default/Cookies or Default/Network/Cookies or Local State
  let markers = [
    profile_dir.join("Default").join("Cookies"),
    profile_dir.join("Default").join("Network").join("Cookies"),
    profile_dir.join("Local State"),
  ];
  markers.iter().any(|m| m.exists())
}

pub fn check_auth_status() -> PlatformAuthStatus {
  let root = resolve_auth_profiles_root();
  let legacy_yt = root
    .parent()
    .unwrap_or_else(|| Path::new("."))
    .join(".youtube_auth");
  PlatformAuthStatus {
    youtube_connected: profile_has_session(&root.join("youtube"))
      || profile_has_session(&legacy_yt),
    tiktok_connected: profile_has_session(&root.join("tiktok")),
    instagram_connected: profile_has_session(&root.join("instagram")),
  }
}

pub async fn open_platform_login(platform: PlatformKind) -> Result<(), String> {
  let script_path = resolve_auto_poster_dir().join(platform.script_name());
  if !script_path.exists() {
    return Err(format!(
      "Auto-poster script not found at {}",
      script_path.display()
    ));
  }

  let output = TokioCommand::new("node")
    .arg(&script_path)
    .arg("--login-only")
    .env("PATH", enhanced_node_path())
    .output()
    .await
    .map_err(|e| format!("Failed to launch Node.js for login: {e}"))?;

  if output.status.success() {
    Ok(())
  } else {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!(
      "Login flow ended with error: {} {}",
      stderr.trim(),
      stdout.trim()
    ))
  }
}

pub async fn execute_platform_publish(
  app: &AppHandle,
  item: &PublishItem,
  platform: PlatformKind,
) -> Result<(), String> {
  let queue = app.state::<PublishQueueState>();
  let exec_lock = app.state::<PublishExecutionLock>();
  let _guard = exec_lock.0.lock().await;

  if !Path::new(&item.video_path).exists() {
    let msg = format!("Video file not found on disk: {}", item.video_path);
    queue.set_platform_status(
      app,
      &item.id,
      platform,
      PublishStatus::Failed,
      Some(msg.clone()),
    );
    return Err(msg);
  }

  let script_path = resolve_auto_poster_dir().join(platform.script_name());
  if !script_path.exists() {
    let msg = format!("Uploader script missing: {}", script_path.display());
    queue.set_platform_status(
      app,
      &item.id,
      platform,
      PublishStatus::Failed,
      Some(msg.clone()),
    );
    return Err(msg);
  }

  queue.set_platform_status(app, &item.id, platform, PublishStatus::Posting, None);

  let meta_payload = json!({
    "videoPath": item.video_path,
    "platform": platform.profile_name(),
    "title": item.youtube_title,
    "description": item.youtube_description,
    "tiktokCaption": item.tiktok_caption,
    "instagramCaption": item.instagram_caption,
    "hashtags": item.hashtags,
    "hookText": item.hook_text,
  });

  let meta_file = std::env::temp_dir().join(format!(
    "viral_automator_meta_{}_{}.json",
    item.id,
    platform.profile_name()
  ));

  tokio::fs::write(&meta_file, meta_payload.to_string())
    .await
    .map_err(|e| format!("Failed to write temp publish metadata: {e}"))?;

  tracing::info!(
    "Launching {} uploader for item {} ({})",
    platform.profile_name(),
    item.id,
    item.video_path
  );

  let output = TokioCommand::new("node")
    .arg(&script_path)
    .arg("--meta")
    .arg(&meta_file)
    .env("PATH", enhanced_node_path())
    .output()
    .await;

  let _ = tokio::fs::remove_file(&meta_file).await;

  match output {
    Ok(out) if out.status.success() => {
      tracing::info!(
        "Successfully published {} to {}",
        item.video_path,
        platform.profile_name()
      );
      queue.set_platform_status(app, &item.id, platform, PublishStatus::Posted, None);
      Ok(())
    }
    Ok(out) => {
      let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
      let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
      let detail = if !stderr.is_empty() {
        stderr
      } else if !stdout.is_empty() {
        stdout
      } else {
        format!("Exited with status {}", out.status)
      };
      tracing::error!(
        "Publish failed for {} on {}: {}",
        item.id,
        platform.profile_name(),
        detail
      );
      queue.set_platform_status(
        app,
        &item.id,
        platform,
        PublishStatus::Failed,
        Some(detail.clone()),
      );
      Err(detail)
    }
    Err(e) => {
      let msg = format!("Failed to spawn Node.js uploader: {e}");
      queue.set_platform_status(
        app,
        &item.id,
        platform,
        PublishStatus::Failed,
        Some(msg.clone()),
      );
      Err(msg)
    }
  }
}

/// Spawns a lightweight background loop checking every 30s for scheduled items ready to drip-post.
pub fn start_drip_scheduler(app: AppHandle) {
  tauri::async_runtime::spawn(async move {
    let mut ticker = tokio::time::interval(Duration::from_secs(30));
    loop {
      ticker.tick().await;
      let queue = app.state::<PublishQueueState>();
      let due = queue.due_jobs(now_epoch_secs());
      for (item_id, platform) in due {
        if let Some(item) = queue.get(&item_id) {
          let _ = execute_platform_publish(&app, &item, platform).await;
        }
      }
    }
  });
}
