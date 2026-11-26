import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type {
  PlatformAuthStatus,
  PlatformKind,
  PublishItem,
  PublishItemUpdate,
} from '../tauri/types/publish.ts';

export const usePublishStore = defineStore('publish', () => {
  const items = ref<PublishItem[]>([]);
  const authStatus = ref<PlatformAuthStatus>({
    youtubeConnected: false,
    tiktokConnected: false,
    instagramConnected: false,
  });
  const isLoading = ref(false);
  const busyItemIds = ref<Set<string>>(new Set());
  const connectingPlatform = ref<PlatformKind | null>(null);

  const pendingCount = computed(() =>
    items.value.filter(item =>
      [item.youtube, item.tiktok, item.instagram].some(
        p => p.enabled && (p.status === 'pending' || p.status === 'scheduled'),
      ),
    ).length,
  );

  const postedCount = computed(() =>
    items.value.filter(item =>
      [item.youtube, item.tiktok, item.instagram].some(p => p.status === 'posted'),
    ).length,
  );

  const scheduledCount = computed(() =>
    items.value.filter(item => item.scheduledAt !== null).length,
  );

  function setItems(next: PublishItem[]) {
    items.value = next;
  }

  async function refresh() {
    isLoading.value = true;
    try {
      const [queue, auth] = await Promise.all([
        invoke<PublishItem[]>('publish_queue_list'),
        invoke<PlatformAuthStatus>('publish_auth_status'),
      ]);
      items.value = queue;
      authStatus.value = auth;
    } finally {
      isLoading.value = false;
    }
  }

  async function refreshAuthStatus() {
    authStatus.value = await invoke<PlatformAuthStatus>('publish_auth_status');
  }

  async function addLocalVideo(videoPath: string, sourceTitle?: string) {
    const item = await invoke<PublishItem>('publish_queue_add', {
      videoPath,
      sourceTitle: sourceTitle ?? null,
      generateAiMetadata: true,
    });
    return item;
  }

  async function updateItem(id: string, patch: PublishItemUpdate) {
    return await invoke<PublishItem>('publish_queue_update', { id, patch });
  }

  async function removeItem(id: string) {
    await invoke('publish_queue_remove', { id });
  }

  async function clearPosted() {
    return await invoke<number>('publish_queue_clear_posted');
  }

  async function autoSchedulePending(intervalHours: number) {
    items.value = await invoke<PublishItem[]>('publish_auto_schedule', {
      intervalHours,
    });
  }

  async function publishNow(id: string, platform?: PlatformKind) {
    busyItemIds.value = new Set([...busyItemIds.value, id]);
    try {
      await invoke('publish_item_now', {
        id,
        platform: platform ?? null,
      });
    } finally {
      const next = new Set(busyItemIds.value);
      next.delete(id);
      busyItemIds.value = next;
    }
  }

  async function regenerateMetadata(id: string) {
    busyItemIds.value = new Set([...busyItemIds.value, id]);
    try {
      return await invoke<PublishItem>('publish_regenerate_metadata', { id });
    } finally {
      const next = new Set(busyItemIds.value);
      next.delete(id);
      busyItemIds.value = next;
    }
  }

  async function connectPlatform(platform: PlatformKind) {
    connectingPlatform.value = platform;
    try {
      await invoke('publish_open_login', { platform });
      await refreshAuthStatus();
    } finally {
      connectingPlatform.value = null;
    }
  }

  return {
    items,
    authStatus,
    isLoading,
    busyItemIds,
    connectingPlatform,
    pendingCount,
    postedCount,
    scheduledCount,
    setItems,
    refresh,
    refreshAuthStatus,
    addLocalVideo,
    updateItem,
    removeItem,
    clearPosted,
    autoSchedulePending,
    publishNow,
    regenerateMetadata,
    connectPlatform,
  };
});
