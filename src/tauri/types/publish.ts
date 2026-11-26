export type PlatformKind = 'youtube' | 'tiktok' | 'instagram';

export type PublishStatus = 'pending' | 'scheduled' | 'posting' | 'posted' | 'failed' | 'skipped';

export interface PlatformPostState {
  enabled: boolean;
  status: PublishStatus;
  lastError: string | null;
  postedAt: number | null;
}

export interface PublishItem {
  id: string;
  videoPath: string;
  thumbnailPath: string | null;
  sourceTitle: string;
  sourceUrl: string | null;
  viralityScore: number | null;
  hookText: string;
  reason: string | null;
  youtubeTitle: string;
  youtubeDescription: string;
  tiktokCaption: string;
  instagramCaption: string;
  hashtags: string[];
  scheduledAt: number | null;
  createdAt: number;
  youtube: PlatformPostState;
  tiktok: PlatformPostState;
  instagram: PlatformPostState;
}

export interface PublishItemUpdate {
  hookText?: string;
  youtubeTitle?: string;
  youtubeDescription?: string;
  tiktokCaption?: string;
  instagramCaption?: string;
  hashtags?: string[];
  scheduledAt?: number | null;
  youtubeEnabled?: boolean;
  tiktokEnabled?: boolean;
  instagramEnabled?: boolean;
}

export interface PlatformAuthStatus {
  youtubeConnected: boolean;
  tiktokConnected: boolean;
  instagramConnected: boolean;
}
