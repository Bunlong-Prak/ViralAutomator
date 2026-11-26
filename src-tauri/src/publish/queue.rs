use crate::publish::models::{PlatformKind, PublishItem, PublishItemUpdate, PublishStatus};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub fn now_epoch_secs() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0)
}

#[derive(Clone)]
pub struct PublishQueueState {
  items: Arc<Mutex<Vec<PublishItem>>>,
  store_path: PathBuf,
}

impl PublishQueueState {
  pub fn new(app_dir: PathBuf) -> Self {
    let store_path = app_dir.join("publish_queue.json");
    let mut loaded: Vec<PublishItem> = std::fs::read_to_string(&store_path)
      .ok()
      .and_then(|raw| serde_json::from_str(&raw).ok())
      .unwrap_or_default();

    // Recover any interrupted "posting" statuses after unexpected app exit
    for item in &mut loaded {
      let has_schedule = item.scheduled_at.is_some();
      for platform in PlatformKind::all() {
        let state = item.platform_state_mut(platform);
        if state.status == PublishStatus::Posting {
          state.status = if has_schedule {
            PublishStatus::Scheduled
          } else {
            PublishStatus::Pending
          };
        }
      }
    }

    Self {
      items: Arc::new(Mutex::new(loaded)),
      store_path,
    }
  }

  fn persist(&self, snapshot: &[PublishItem]) {
    if let Some(parent) = self.store_path.parent() {
      let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(snapshot) {
      let tmp = self.store_path.with_extension("json.tmp");
      if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &self.store_path);
      }
    }
  }

  fn emit_updated(&self, app: &AppHandle, snapshot: &[PublishItem]) {
    let _ = app.emit("publish_queue_updated", snapshot);
  }

  pub fn list(&self) -> Vec<PublishItem> {
    self.items.lock().map(|g| g.clone()).unwrap_or_default()
  }

  pub fn get(&self, id: &str) -> Option<PublishItem> {
    self
      .items
      .lock()
      .ok()
      .and_then(|g| g.iter().find(|i| i.id == id).cloned())
  }

  pub fn add(&self, app: &AppHandle, item: PublishItem) -> PublishItem {
    let snapshot = {
      let mut guard = self.items.lock().unwrap();
      guard.insert(0, item.clone());
      guard.clone()
    };
    self.persist(&snapshot);
    self.emit_updated(app, &snapshot);
    item
  }

  pub fn update(
    &self,
    app: &AppHandle,
    id: &str,
    patch: PublishItemUpdate,
  ) -> Result<PublishItem, String> {
    let (updated, snapshot) = {
      let mut guard = self.items.lock().unwrap();
      let item = guard
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("Publish queue item not found: {id}"))?;

      if let Some(v) = patch.hook_text {
        item.hook_text = v;
      }
      if let Some(v) = patch.youtube_title {
        item.youtube_title = v;
      }
      if let Some(v) = patch.youtube_description {
        item.youtube_description = v;
      }
      if let Some(v) = patch.tiktok_caption {
        item.tiktok_caption = v;
      }
      if let Some(v) = patch.instagram_caption {
        item.instagram_caption = v;
      }
      if let Some(v) = patch.hashtags {
        item.hashtags = v;
      }
      if let Some(sched) = patch.scheduled_at {
        item.scheduled_at = sched;
        for platform in PlatformKind::all() {
          let state = item.platform_state_mut(platform);
          if state.enabled && state.status != PublishStatus::Posted {
            state.status = if sched.is_some() {
              PublishStatus::Scheduled
            } else {
              PublishStatus::Pending
            };
          }
        }
      }

      for (platform, flag) in [
        (PlatformKind::Youtube, patch.youtube_enabled),
        (PlatformKind::Tiktok, patch.tiktok_enabled),
        (PlatformKind::Instagram, patch.instagram_enabled),
      ] {
        if let Some(enabled) = flag {
          let has_schedule = item.scheduled_at.is_some();
          let state = item.platform_state_mut(platform);
          state.enabled = enabled;
          if state.status != PublishStatus::Posted && state.status != PublishStatus::Posting {
            state.status = if !enabled {
              PublishStatus::Skipped
            } else if has_schedule {
              PublishStatus::Scheduled
            } else {
              PublishStatus::Pending
            };
          }
        }
      }

      (item.clone(), guard.clone())
    };

    self.persist(&snapshot);
    self.emit_updated(app, &snapshot);
    Ok(updated)
  }

  pub fn set_platform_status(
    &self,
    app: &AppHandle,
    id: &str,
    platform: PlatformKind,
    status: PublishStatus,
    error: Option<String>,
  ) {
    let snapshot = {
      let mut guard = self.items.lock().unwrap();
      if let Some(item) = guard.iter_mut().find(|i| i.id == id) {
        let state = item.platform_state_mut(platform);
        state.status = status;
        state.last_error = error;
        if status == PublishStatus::Posted {
          state.posted_at = Some(now_epoch_secs());
        }
      }
      guard.clone()
    };
    self.persist(&snapshot);
    self.emit_updated(app, &snapshot);
  }

  pub fn remove(&self, app: &AppHandle, id: &str) {
    let snapshot = {
      let mut guard = self.items.lock().unwrap();
      guard.retain(|i| i.id != id);
      guard.clone()
    };
    self.persist(&snapshot);
    self.emit_updated(app, &snapshot);
  }

  pub fn clear_posted(&self, app: &AppHandle) -> usize {
    let (removed, snapshot) = {
      let mut guard = self.items.lock().unwrap();
      let before = guard.len();
      guard.retain(|i| !i.is_fully_posted());
      (before - guard.len(), guard.clone())
    };
    self.persist(&snapshot);
    self.emit_updated(app, &snapshot);
    removed
  }

  /// Auto-staggers all unscheduled pending items by `interval_hours` so platforms don't flag spam bursts.
  pub fn auto_schedule_pending(&self, app: &AppHandle, interval_hours: u32) -> Vec<PublishItem> {
    let now = now_epoch_secs();
    let step_secs = u64::from(interval_hours.max(1)) * 3600;

    let snapshot = {
      let mut guard = self.items.lock().unwrap();
      let mut next_slot = guard
        .iter()
        .filter_map(|i| i.scheduled_at)
        .filter(|&ts| ts > now)
        .max()
        .map(|latest| latest + step_secs)
        .unwrap_or(now + 120); // 2 minutes from now for the very first slot

      // Schedule highest-virality items first
      let mut indices: Vec<usize> = guard
        .iter()
        .enumerate()
        .filter(|(_, item)| item.scheduled_at.is_none() && item.has_unposted_enabled_platform())
        .map(|(idx, _)| idx)
        .collect();
      indices.sort_by(|&a, &b| {
        guard[b]
          .virality_score
          .unwrap_or(0)
          .cmp(&guard[a].virality_score.unwrap_or(0))
      });

      for idx in indices {
        let item = &mut guard[idx];
        item.scheduled_at = Some(next_slot);
        for platform in PlatformKind::all() {
          let state = item.platform_state_mut(platform);
          if state.enabled && state.status == PublishStatus::Pending {
            state.status = PublishStatus::Scheduled;
          }
        }
        next_slot += step_secs;
      }

      guard.clone()
    };

    self.persist(&snapshot);
    self.emit_updated(app, &snapshot);
    snapshot
  }

  pub fn next_drip_slot(&self, interval_hours: u32) -> u64 {
    let now = now_epoch_secs();
    let step_secs = u64::from(interval_hours.max(1)) * 3600;
    let guard = self.items.lock().unwrap();
    guard
      .iter()
      .filter_map(|i| i.scheduled_at)
      .filter(|&ts| ts > now)
      .max()
      .map(|latest| latest + step_secs)
      .unwrap_or(now + 300)
  }

  pub fn due_jobs(&self, now_secs: u64) -> Vec<(String, PlatformKind)> {
    let guard = self.items.lock().unwrap();
    let mut jobs = Vec::new();
    for item in guard.iter() {
      let Some(sched) = item.scheduled_at else {
        continue;
      };
      if sched > now_secs {
        continue;
      }
      for platform in PlatformKind::all() {
        let state = item.platform_state(platform);
        if state.enabled && state.status == PublishStatus::Scheduled {
          jobs.push((item.id.clone(), platform));
        }
      }
    }
    jobs
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::publish::models::PlatformPostState;

  fn sample_item(id: &str, score: u8, scheduled_at: Option<u64>) -> PublishItem {
    PublishItem {
      id: id.into(),
      video_path: format!("/tmp/{id}.mp4"),
      thumbnail_path: None,
      source_title: format!("Clip {id}"),
      source_url: None,
      virality_score: Some(score),
      hook_text: "WATCH THIS".into(),
      reason: None,
      youtube_title: format!("YT {id}"),
      youtube_description: "desc".into(),
      tiktok_caption: "tt".into(),
      instagram_caption: "ig".into(),
      hashtags: vec!["viral".into()],
      scheduled_at,
      created_at: 1000,
      youtube: PlatformPostState::new(true),
      tiktok: PlatformPostState::new(true),
      instagram: PlatformPostState::new(false),
    }
  }

  #[test]
  fn due_jobs_returns_only_enabled_scheduled_items_past_due() {
    let dir = std::env::temp_dir().join(format!("va_pub_test_{}", uuid::Uuid::new_v4()));
    let state = PublishQueueState::new(dir.clone());
    let mut due_item = sample_item("due", 95, Some(500));
    due_item.youtube.status = PublishStatus::Scheduled;
    due_item.tiktok.status = PublishStatus::Posted;
    let mut future_item = sample_item("future", 80, Some(5000));
    future_item.youtube.status = PublishStatus::Scheduled;

    {
      let mut g = state.items.lock().unwrap();
      g.push(due_item);
      g.push(future_item);
    }

    let jobs = state.due_jobs(1000);
    assert_eq!(jobs, vec![("due".to_string(), PlatformKind::Youtube)]);
    let _ = std::fs::remove_dir_all(dir);
  }
}
