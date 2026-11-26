<template>
  <div class="studio-page flex flex-col min-h-full">
    <base-sub-nav>
      <template #title>
        <h1 class="text-lg font-semibold self-center">
          {{ t('studio.title') }}
        </h1>
      </template>
      <template #default>
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="btn btn-sm btn-soft gap-1"
            @click="addLocalClip"
          >
            <plus-icon class="w-4 h-4" aria-hidden="true" />
            {{ t('studio.actions.addVideo') }}
          </button>
          <button
            type="button"
            class="btn btn-sm btn-soft gap-1"
            :disabled="publishStore.postedCount === 0"
            @click="clearPostedClips"
          >
            <trash-icon class="w-4 h-4" aria-hidden="true" />
            {{ t('studio.actions.clearPosted') }}
          </button>
        </div>
      </template>
    </base-sub-nav>

    <div class="studio-container flex flex-col gap-6 py-5 px-6">
      <!-- Platform Accounts & Drip Scheduler Control Bar -->
      <section class="grid gap-4 lg:grid-cols-2">
        <div class="bg-base-200 border border-base-300 rounded-box p-4 flex flex-col gap-3">
          <div class="flex items-center justify-between gap-2">
            <div>
              <h2 class="font-semibold text-base">
                {{ t('studio.accounts.title') }}
              </h2>
              <p class="text-xs opacity-70">
                {{ t('studio.accounts.hint') }}
              </p>
            </div>
            <button
              type="button"
              class="btn btn-xs btn-ghost"
              @click="publishStore.refreshAuthStatus()"
            >
              {{ t('studio.accounts.refresh') }}
            </button>
          </div>

          <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
            <div
              v-for="platform in platformDefs"
              :key="platform.id"
              class="bg-base-100 rounded-field p-3 border border-base-300 flex flex-col justify-between gap-2"
            >
              <div class="flex items-center justify-between gap-1">
                <span class="font-semibold text-sm">{{ t(platform.labelKey) }}</span>
                <span
                  class="badge badge-xs"
                  :class="isConnected(platform.id) ? 'badge-success' : 'badge-ghost'"
                >
                  {{ isConnected(platform.id) ? t('studio.accounts.connected') : t('studio.accounts.notConnected') }}
                </span>
              </div>
              <button
                type="button"
                class="btn btn-xs w-full"
                :class="isConnected(platform.id) ? 'btn-soft' : 'btn-primary'"
                :disabled="publishStore.connectingPlatform !== null"
                @click="connectAccount(platform.id)"
              >
                <span
                  v-if="publishStore.connectingPlatform === platform.id"
                  class="loading loading-spinner loading-xs"
                  aria-hidden="true"
                />
                {{ isConnected(platform.id) ? t('studio.accounts.reconnect') : t('studio.accounts.connect') }}
              </button>
            </div>
          </div>
        </div>

        <div class="bg-base-200 border border-base-300 rounded-box p-4 flex flex-col justify-between gap-3">
          <div>
            <h2 class="font-semibold text-base">
              {{ t('studio.scheduler.title') }}
            </h2>
            <p class="text-xs opacity-70">
              {{ t('studio.scheduler.hint') }}
            </p>
          </div>

          <div class="flex flex-wrap items-end gap-3">
            <div class="flex flex-col gap-1 grow min-w-40">
              <label for="drip-interval-select" class="text-xs font-semibold">
                {{ t('studio.scheduler.intervalLabel') }}
              </label>
              <select
                id="drip-interval-select"
                v-model.number="dripIntervalHours"
                class="select select-sm select-bordered w-full"
              >
                <option :value="2">
                  {{ t('studio.scheduler.everyHours', { n: 2 }) }}
                </option>
                <option :value="4">
                  {{ t('studio.scheduler.everyHours', { n: 4 }) }}
                </option>
                <option :value="6">
                  {{ t('studio.scheduler.everyHours', { n: 6 }) }}
                </option>
                <option :value="8">
                  {{ t('studio.scheduler.everyHours', { n: 8 }) }}
                </option>
                <option :value="12">
                  {{ t('studio.scheduler.everyHours', { n: 12 }) }}
                </option>
                <option :value="24">
                  {{ t('studio.scheduler.everyHours', { n: 24 }) }}
                </option>
              </select>
            </div>

            <button
              type="button"
              class="btn btn-sm btn-primary gap-1"
              :disabled="publishStore.pendingCount === 0"
              @click="autoScheduleAll"
            >
              <clock-icon class="w-4 h-4" aria-hidden="true" />
              {{ t('studio.scheduler.autoSchedule') }}
            </button>
          </div>

          <div class="flex flex-wrap gap-3 text-xs pt-1" aria-live="polite">
            <span class="badge badge-neutral">
              {{ t('studio.stats.total', { n: publishStore.items.length }) }}
            </span>
            <span class="badge badge-info badge-soft">
              {{ t('studio.stats.scheduled', { n: publishStore.scheduledCount }) }}
            </span>
            <span class="badge badge-success badge-soft">
              {{ t('studio.stats.posted', { n: publishStore.postedCount }) }}
            </span>
          </div>
        </div>
      </section>

      <!-- Empty State -->
      <section
        v-if="!publishStore.isLoading && publishStore.items.length === 0"
        class="bg-base-200 border border-dashed border-base-300 rounded-box p-10 text-center flex flex-col items-center gap-3"
      >
        <rocket-launch-icon class="w-10 h-10 opacity-60" aria-hidden="true" />
        <h2 class="text-lg font-semibold">
          {{ t('studio.empty.title') }}
        </h2>
        <p class="text-sm opacity-75 max-w-lg">
          {{ t('studio.empty.subtitle') }}
        </p>
        <div class="flex flex-wrap justify-center gap-2 mt-2">
          <button type="button" class="btn btn-primary btn-sm" @click="addLocalClip">
            {{ t('studio.actions.addVideo') }}
          </button>
          <router-link :to="{ name: 'settings.downloads' }" class="btn btn-soft btn-sm">
            {{ t('studio.empty.configureFaceless') }}
          </router-link>
        </div>
      </section>

      <!-- Responsive Clip Queue Grid -->
      <section v-else class="studio-grid">
        <article
          v-for="item in publishStore.items"
          :key="item.id"
          class="studio-card bg-base-200 border border-base-300 rounded-box overflow-clip flex flex-col"
        >
          <div class="studio-card-media relative bg-base-300">
            <img
              v-if="item.thumbnailPath"
              :src="toAssetUrl(item.thumbnailPath)"
              :alt="item.youtubeTitle"
              class="w-full h-full object-cover block"
            />
            <div
              v-else
              class="w-full h-full flex items-center justify-center bg-gradient-to-br from-primary/20 to-secondary/20"
            >
              <film-icon class="w-10 h-10 opacity-50" aria-hidden="true" />
            </div>

            <span
              v-if="item.viralityScore !== null"
              class="badge badge-warning font-bold absolute top-2 right-2 shadow"
            >
              {{ t('studio.card.viralScore', { score: item.viralityScore }) }}
            </span>

            <div
              v-if="item.hookText"
              class="absolute bottom-2 inset-x-2 bg-base-100/90 backdrop-blur-xs rounded-field px-2.5 py-1 text-xs font-extrabold tracking-wide text-center truncate"
            >
              {{ item.hookText }}
            </div>
          </div>

          <div class="p-4 flex flex-col gap-3 grow min-w-0">
            <header class="flex flex-col gap-1 min-w-0">
              <h2 class="font-semibold text-sm leading-snug line-clamp-2" :title="item.youtubeTitle">
                {{ item.youtubeTitle }}
              </h2>
              <p v-if="item.scheduledAt" class="text-xs text-info flex items-center gap-1">
                <clock-icon class="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
                {{ t('studio.card.scheduledFor', { time: formatEpoch(item.scheduledAt) }) }}
              </p>
              <p v-else class="text-xs opacity-65">
                {{ t('studio.card.unscheduled') }}
              </p>
            </header>

            <!-- Per-Platform Status Pills -->
            <div class="flex flex-col gap-1.5">
              <div
                v-for="platform in platformDefs"
                :key="platform.id"
                class="flex items-center justify-between gap-2 bg-base-100 rounded-field px-2.5 py-1.5 text-xs"
              >
                <span class="font-medium">{{ t(platform.shortLabelKey) }}</span>
                <div class="flex items-center gap-1.5 min-w-0">
                  <span
                    class="badge badge-xs"
                    :class="statusBadgeClass(item[platform.id].status)"
                    :title="item[platform.id].lastError ?? ''"
                  >
                    {{ t(`studio.status.${item[platform.id].status}`) }}
                  </span>
                  <button
                    v-if="item[platform.id].enabled && item[platform.id].status !== 'posted'"
                    type="button"
                    class="btn btn-xs btn-ghost px-1.5 min-h-6"
                    :disabled="publishStore.busyItemIds.has(item.id)"
                    :aria-label="t('studio.actions.postSinglePlatform', { platform: t(platform.labelKey), title: item.youtubeTitle })"
                    @click="publishSingle(item.id, platform.id)"
                  >
                    <paper-airplane-icon class="w-3.5 h-3.5" aria-hidden="true" />
                  </button>
                </div>
              </div>
            </div>

            <footer class="mt-auto pt-2 border-t border-base-300 flex flex-wrap items-center gap-2">
              <button
                type="button"
                class="btn btn-primary btn-xs grow min-h-8 gap-1"
                :disabled="publishStore.busyItemIds.has(item.id)"
                :aria-label="t('studio.actions.publishAllFor', { title: item.youtubeTitle })"
                @click="publishAll(item.id)"
              >
                <span
                  v-if="publishStore.busyItemIds.has(item.id)"
                  class="loading loading-spinner loading-xs"
                  aria-hidden="true"
                />
                <paper-airplane-icon v-else class="w-3.5 h-3.5" aria-hidden="true" />
                {{ t('studio.actions.publishNow') }}
              </button>
              <button
                type="button"
                class="btn btn-soft btn-xs min-h-8"
                :aria-label="t('studio.actions.editFor', { title: item.youtubeTitle })"
                @click="openEditor(item)"
              >
                {{ t('studio.actions.edit') }}
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-xs min-h-8"
                :aria-label="t('studio.actions.openFolderFor', { title: item.youtubeTitle })"
                @click="revealVideo(item.videoPath)"
              >
                <folder-icon class="w-4 h-4" aria-hidden="true" />
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-xs text-error min-h-8"
                :aria-label="t('studio.actions.removeFor', { title: item.youtubeTitle })"
                @click="removeClip(item.id)"
              >
                <trash-icon class="w-4 h-4" aria-hidden="true" />
              </button>
            </footer>
          </div>
        </article>
      </section>
    </div>

    <!-- Native Modal Dialog for Editing Multi-Platform Metadata & Schedule -->
    <dialog
      ref="editorDialogRef"
      class="studio-dialog bg-base-100 text-base-content rounded-box border border-base-300 shadow-2xl p-0 w-full max-w-2xl"
      aria-labelledby="publish-editor-title"
      @close="onDialogClose"
    >
      <form v-if="editingItem" class="flex flex-col max-h-[85dvh]" @submit.prevent="saveEditor">
        <header class="px-6 py-4 border-b border-base-300 flex items-center justify-between gap-2">
          <h1 id="publish-editor-title" class="text-lg font-bold">
            {{ t('studio.editor.title') }}
          </h1>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="btn btn-xs btn-secondary gap-1"
              :disabled="isRegenerating"
              @click="regenerateWithAi"
            >
              <span v-if="isRegenerating" class="loading loading-spinner loading-xs" aria-hidden="true" />
              <sparkles-icon v-else class="w-3.5 h-3.5" aria-hidden="true" />
              {{ t('studio.editor.regenerateAi') }}
            </button>
            <button
              type="button"
              class="btn btn-xs btn-circle btn-ghost"
              :aria-label="t('studio.editor.close')"
              @click="closeEditor"
            >
              <x-mark-icon class="w-4 h-4" aria-hidden="true" />
            </button>
          </div>
        </header>

        <div class="p-6 overflow-y-auto flex flex-col gap-4">
          <!-- Target Platforms -->
          <fieldset class="flex flex-wrap gap-4">
            <legend class="text-sm font-semibold mb-2">
              {{ t('studio.editor.platforms') }}
            </legend>
            <label class="label cursor-pointer gap-2">
              <input v-model="draft.youtubeEnabled" type="checkbox" class="checkbox checkbox-primary checkbox-sm" />
              <span class="label-text">{{ t('studio.platforms.youtube') }}</span>
            </label>
            <label class="label cursor-pointer gap-2">
              <input v-model="draft.tiktokEnabled" type="checkbox" class="checkbox checkbox-primary checkbox-sm" />
              <span class="label-text">{{ t('studio.platforms.tiktok') }}</span>
            </label>
            <label class="label cursor-pointer gap-2">
              <input v-model="draft.instagramEnabled" type="checkbox" class="checkbox checkbox-primary checkbox-sm" />
              <span class="label-text">{{ t('studio.platforms.instagram') }}</span>
            </label>
          </fieldset>

          <!-- 3-Second Viral Hook -->
          <div class="flex flex-col gap-1">
            <label for="editor-hook" class="text-sm font-semibold">
              {{ t('studio.editor.hookLabel') }}
            </label>
            <input
              id="editor-hook"
              v-model="draft.hookText"
              type="text"
              autofocus
              class="input input-bordered w-full"
            />
          </div>

          <!-- YouTube Shorts -->
          <div class="flex flex-col gap-1">
            <label for="editor-yt-title" class="text-sm font-semibold">
              {{ t('studio.editor.youtubeTitle') }}
            </label>
            <input
              id="editor-yt-title"
              v-model="draft.youtubeTitle"
              type="text"
              maxlength="100"
              class="input input-bordered w-full"
            />
          </div>

          <div class="flex flex-col gap-1">
            <label for="editor-yt-desc" class="text-sm font-semibold">
              {{ t('studio.editor.youtubeDescription') }}
            </label>
            <textarea
              id="editor-yt-desc"
              v-model="draft.youtubeDescription"
              rows="3"
              class="textarea textarea-bordered w-full"
            />
          </div>

          <!-- TikTok Caption -->
          <div class="flex flex-col gap-1">
            <label for="editor-tt-caption" class="text-sm font-semibold">
              {{ t('studio.editor.tiktokCaption') }}
            </label>
            <textarea
              id="editor-tt-caption"
              v-model="draft.tiktokCaption"
              rows="2"
              class="textarea textarea-bordered w-full"
            />
          </div>

          <!-- Instagram Reels Caption -->
          <div class="flex flex-col gap-1">
            <label for="editor-ig-caption" class="text-sm font-semibold">
              {{ t('studio.editor.instagramCaption') }}
            </label>
            <textarea
              id="editor-ig-caption"
              v-model="draft.instagramCaption"
              rows="3"
              class="textarea textarea-bordered w-full"
            />
          </div>

          <!-- Schedule Date/Time -->
          <div class="flex flex-col gap-1">
            <label for="editor-schedule" class="text-sm font-semibold">
              {{ t('studio.editor.scheduleAt') }}
            </label>
            <div class="flex items-center gap-2">
              <input
                id="editor-schedule"
                v-model="draft.scheduledLocal"
                type="datetime-local"
                class="input input-bordered grow"
              />
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :disabled="!draft.scheduledLocal"
                @click="draft.scheduledLocal = ''"
              >
                {{ t('studio.editor.clearSchedule') }}
              </button>
            </div>
          </div>
        </div>

        <footer class="px-6 py-4 border-t border-base-300 flex justify-end gap-2">
          <button type="button" class="btn btn-ghost btn-sm" @click="closeEditor">
            {{ t('studio.editor.cancel') }}
          </button>
          <button type="submit" class="btn btn-primary btn-sm">
            {{ t('common.save') }}
          </button>
        </footer>
      </form>
    </dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { convertFileSrc } from '@tauri-apps/api/core';
import {
  ClockIcon,
  FilmIcon,
  FolderIcon,
  PaperAirplaneIcon,
  PlusIcon,
  RocketLaunchIcon,
  SparklesIcon,
  TrashIcon,
  XMarkIcon,
} from '@heroicons/vue/24/outline';
import BaseSubNav from '../../components/base/BaseSubNav.vue';
import { usePublishStore } from '../../stores/publish.ts';
import { useSettingsStore } from '../../stores/settings.ts';
import { useToastStore } from '../../stores/toast.ts';
import { useDialog } from '../../composables/useDialog.ts';
import { useOpener } from '../../composables/useOpener.ts';
import type { PlatformKind, PublishItem, PublishStatus } from '../../tauri/types/publish.ts';

const { t } = useI18n();
const publishStore = usePublishStore();
const settingsStore = useSettingsStore();
const toastStore = useToastStore();
const { openPathDialog } = useDialog();
const { revealPath } = useOpener();

const dripIntervalHours = ref<number>(
  settingsStore.settings.output.video.uniqueness.dripIntervalHours || 4,
);

const platformDefs: { id: PlatformKind; labelKey: string; shortLabelKey: string }[] = [
  { id: 'youtube', labelKey: 'studio.platforms.youtube', shortLabelKey: 'studio.platforms.youtubeShort' },
  { id: 'tiktok', labelKey: 'studio.platforms.tiktok', shortLabelKey: 'studio.platforms.tiktok' },
  { id: 'instagram', labelKey: 'studio.platforms.instagram', shortLabelKey: 'studio.platforms.instagramShort' },
];

const editorDialogRef = ref<HTMLDialogElement | null>(null);
const editingItem = ref<PublishItem | null>(null);
const isRegenerating = ref(false);

const draft = reactive({
  hookText: '',
  youtubeTitle: '',
  youtubeDescription: '',
  tiktokCaption: '',
  instagramCaption: '',
  scheduledLocal: '',
  youtubeEnabled: true,
  tiktokEnabled: true,
  instagramEnabled: true,
});

onMounted(() => {
  void publishStore.refresh();
});

function isConnected(platform: PlatformKind): boolean {
  if (platform === 'youtube') return publishStore.authStatus.youtubeConnected;
  if (platform === 'tiktok') return publishStore.authStatus.tiktokConnected;
  return publishStore.authStatus.instagramConnected;
}

function toAssetUrl(filePath: string): string {
  try {
    return convertFileSrc(filePath);
  } catch {
    return filePath;
  }
}

function statusBadgeClass(status: PublishStatus): string {
  switch (status) {
    case 'posted':
      return 'badge-success';
    case 'posting':
      return 'badge-warning';
    case 'scheduled':
      return 'badge-info';
    case 'failed':
      return 'badge-error';
    case 'skipped':
      return 'badge-ghost opacity-50';
    default:
      return 'badge-neutral';
  }
}

function formatEpoch(epochSecs: number): string {
  return new Date(epochSecs * 1000).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function epochToLocalInput(epochSecs: number | null): string {
  if (!epochSecs) return '';
  const d = new Date(epochSecs * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

async function addLocalClip() {
  const selected = await openPathDialog(false, false, [
    { name: 'Video', extensions: ['mp4', 'mov', 'mkv', 'webm'] },
  ]);
  if (typeof selected !== 'string') return;
  try {
    await publishStore.addLocalVideo(selected);
    toastStore.showToast(t('studio.toasts.added'), { style: 'success' });
  } catch (err) {
    toastStore.showToast(String(err), { style: 'error' });
  }
}

async function clearPostedClips() {
  const removed = await publishStore.clearPosted();
  toastStore.showToast(t('studio.toasts.clearedPosted', { n: removed }), { style: 'info' });
}

async function autoScheduleAll() {
  await publishStore.autoSchedulePending(dripIntervalHours.value);
  toastStore.showToast(
    t('studio.toasts.scheduled', { hours: dripIntervalHours.value }),
    { style: 'success' },
  );
}

async function connectAccount(platform: PlatformKind) {
  toastStore.showToast(t('studio.toasts.openingLogin'), { style: 'info' });
  try {
    await publishStore.connectPlatform(platform);
    toastStore.showToast(t('studio.toasts.connected'), { style: 'success' });
  } catch (err) {
    toastStore.showToast(String(err), { style: 'error' });
  }
}

async function publishAll(id: string) {
  try {
    await publishStore.publishNow(id);
    toastStore.showToast(t('studio.toasts.published'), { style: 'success' });
  } catch (err) {
    toastStore.showToast(String(err), { style: 'error' });
  }
}

async function publishSingle(id: string, platform: PlatformKind) {
  try {
    await publishStore.publishNow(id, platform);
    toastStore.showToast(t('studio.toasts.published'), { style: 'success' });
  } catch (err) {
    toastStore.showToast(String(err), { style: 'error' });
  }
}

async function revealVideo(videoPath: string) {
  await revealPath(videoPath);
}

async function removeClip(id: string) {
  await publishStore.removeItem(id);
}

function openEditor(item: PublishItem) {
  editingItem.value = item;
  draft.hookText = item.hookText;
  draft.youtubeTitle = item.youtubeTitle;
  draft.youtubeDescription = item.youtubeDescription;
  draft.tiktokCaption = item.tiktokCaption;
  draft.instagramCaption = item.instagramCaption;
  draft.scheduledLocal = epochToLocalInput(item.scheduledAt);
  draft.youtubeEnabled = item.youtube.enabled;
  draft.tiktokEnabled = item.tiktok.enabled;
  draft.instagramEnabled = item.instagram.enabled;
  editorDialogRef.value?.showModal();
}

function closeEditor() {
  editorDialogRef.value?.close();
}

function onDialogClose() {
  if (editorDialogRef.value) {
    editorDialogRef.value.returnValue = '';
  }
  editingItem.value = null;
}

async function saveEditor() {
  if (!editingItem.value) return;
  const scheduledAt = draft.scheduledLocal
    ? Math.floor(new Date(draft.scheduledLocal).getTime() / 1000)
    : null;
  await publishStore.updateItem(editingItem.value.id, {
    hookText: draft.hookText,
    youtubeTitle: draft.youtubeTitle,
    youtubeDescription: draft.youtubeDescription,
    tiktokCaption: draft.tiktokCaption,
    instagramCaption: draft.instagramCaption,
    scheduledAt,
    youtubeEnabled: draft.youtubeEnabled,
    tiktokEnabled: draft.tiktokEnabled,
    instagramEnabled: draft.instagramEnabled,
  });
  toastStore.showToast(t('studio.toasts.updated'), { style: 'success' });
  closeEditor();
}

async function regenerateWithAi() {
  if (!editingItem.value) return;
  isRegenerating.value = true;
  try {
    const updated = await publishStore.regenerateMetadata(editingItem.value.id);
    draft.hookText = updated.hookText;
    draft.youtubeTitle = updated.youtubeTitle;
    draft.youtubeDescription = updated.youtubeDescription;
    draft.tiktokCaption = updated.tiktokCaption;
    draft.instagramCaption = updated.instagramCaption;
    toastStore.showToast(t('studio.toasts.regenerated'), { style: 'success' });
  } catch (err) {
    toastStore.showToast(String(err), { style: 'error' });
  } finally {
    isRegenerating.value = false;
  }
}
</script>

<style scoped>
.studio-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 1.25rem;
}

.studio-card {
  container-type: inline-size;
  min-inline-size: 0;
}

.studio-card-media {
  aspect-ratio: 16 / 9;
  inline-size: 100%;
}

.studio-dialog[open] {
  margin: auto;
}

.studio-dialog:modal {
  overscroll-behavior: contain;
}

.studio-dialog::backdrop {
  background: color-mix(in oklab, black 60%, transparent);
  overflow: hidden;
  overscroll-behavior: contain;
}
</style>
