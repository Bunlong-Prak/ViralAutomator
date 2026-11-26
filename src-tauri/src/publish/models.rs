use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformKind {
  Youtube,
  Tiktok,
  Instagram,
}

impl PlatformKind {
  pub fn script_name(self) -> &'static str {
    match self {
      Self::Youtube => "youtube_shorts.js",
      Self::Tiktok => "tiktok.js",
      Self::Instagram => "instagram_reels.js",
    }
  }

  pub fn profile_name(self) -> &'static str {
    match self {
      Self::Youtube => "youtube",
      Self::Tiktok => "tiktok",
      Self::Instagram => "instagram",
    }
  }

  pub fn all() -> [Self; 3] {
    [Self::Youtube, Self::Tiktok, Self::Instagram]
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublishStatus {
  Pending,
  Scheduled,
  Posting,
  Posted,
  Failed,
  Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformPostState {
  pub enabled: bool,
  pub status: PublishStatus,
  pub last_error: Option<String>,
  pub posted_at: Option<u64>,
}

impl PlatformPostState {
  pub fn new(enabled: bool) -> Self {
    Self {
      enabled,
      status: if enabled {
        PublishStatus::Pending
      } else {
        PublishStatus::Skipped
      },
      last_error: None,
      posted_at: None,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishItem {
  pub id: String,
  pub video_path: String,
  pub thumbnail_path: Option<String>,
  pub source_title: String,
  pub source_url: Option<String>,
  pub virality_score: Option<u8>,
  pub hook_text: String,
  pub reason: Option<String>,
  pub youtube_title: String,
  pub youtube_description: String,
  pub tiktok_caption: String,
  pub instagram_caption: String,
  pub hashtags: Vec<String>,
  pub scheduled_at: Option<u64>,
  pub created_at: u64,
  pub youtube: PlatformPostState,
  pub tiktok: PlatformPostState,
  pub instagram: PlatformPostState,
}

impl PublishItem {
  pub fn platform_state(&self, platform: PlatformKind) -> &PlatformPostState {
    match platform {
      PlatformKind::Youtube => &self.youtube,
      PlatformKind::Tiktok => &self.tiktok,
      PlatformKind::Instagram => &self.instagram,
    }
  }

  pub fn platform_state_mut(&mut self, platform: PlatformKind) -> &mut PlatformPostState {
    match platform {
      PlatformKind::Youtube => &mut self.youtube,
      PlatformKind::Tiktok => &mut self.tiktok,
      PlatformKind::Instagram => &mut self.instagram,
    }
  }

  pub fn has_unposted_enabled_platform(&self) -> bool {
    PlatformKind::all().into_iter().any(|p| {
      let state = self.platform_state(p);
      state.enabled
        && matches!(
          state.status,
          PublishStatus::Pending | PublishStatus::Scheduled | PublishStatus::Failed
        )
    })
  }

  pub fn is_fully_posted(&self) -> bool {
    let any_enabled = PlatformKind::all()
      .into_iter()
      .any(|p| self.platform_state(p).enabled);
    if !any_enabled {
      return false;
    }
    PlatformKind::all().into_iter().all(|p| {
      let s = self.platform_state(p);
      !s.enabled || s.status == PublishStatus::Posted
    })
  }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishItemUpdate {
  pub hook_text: Option<String>,
  pub youtube_title: Option<String>,
  pub youtube_description: Option<String>,
  pub tiktok_caption: Option<String>,
  pub instagram_caption: Option<String>,
  pub hashtags: Option<Vec<String>>,
  pub scheduled_at: Option<Option<u64>>,
  pub youtube_enabled: Option<bool>,
  pub tiktok_enabled: Option<bool>,
  pub instagram_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAuthStatus {
  pub youtube_connected: bool,
  pub tiktok_connected: bool,
  pub instagram_connected: bool,
}
