<template>
  <div class="flex flex-col">
    <base-tabbed-pane
        v-model="activeTab"
        :tabs="tabs"
        :id-prefix="tabIdPrefix"
    >
      <template #video>
        <h2 class="sr-only">
          {{ t('settings.output.tabs.video.screenreader') }}
        </h2>

        <base-select
            :label="t('settings.output.tabs.video.container.label')"
            :hint="t('settings.output.tabs.video.container.hint')"
            :options="VideoContainer"
            :locale-key="'settings.output.tabs.video.container.options'"
            v-model="outputState.video.container"
            class="mb-4"
        />

        <div class="mb-4 flex flex-col gap-1">
          <label class="font-semibold" :for="fieldId('keep-original-streams-video')">
            {{ t('settings.output.keepOriginalStreams.label') }}
          </label>
          <input
              :id="fieldId('keep-original-streams-video')"
              type="checkbox"
              v-model="keepOriginalStreamsVideo"
              class="toggle toggle-primary my-1"
          />
          <p class="label">
            {{ t('settings.output.keepOriginalStreams.hint') }}
          </p>
        </div>

        <base-select
            :label="t('settings.output.postprocess.label')"
            :hint="postprocessHint"
            :options="VideoPostprocessPreset"
            :locale-key="'settings.output.postprocess.options'"
            v-model="outputState.video.postprocessPreset"
        />

        <base-select
            v-if="showVideoPostprocessMode"
            :label="t('settings.output.postprocessMode.label')"
            :hint="t('settings.output.postprocessMode.hint')"
            :options="VideoPostprocessMode"
            :locale-key="'settings.output.postprocessMode.options'"
            v-model="outputState.video.customPostprocessMode"
            class="mt-4"
        />

        <template v-if="showVideoPostprocessArgs">
          <label class="mb-2 mt-4 font-semibold" :for="fieldId('video-postprocess-args')">
            {{ t('settings.output.postprocessArgs.label') }}
          </label>
          <textarea
              :id="fieldId('video-postprocess-args')"
              v-model="outputState.video.postprocessArgs"
              class="textarea rounded-xl w-full"
              rows="3"
              :placeholder="t('settings.output.postprocessArgs.hint')"
          />
        </template>

        <p v-if="showMp42ContainerConflict" class="alert alert-error mt-2 alert-soft">
          {{ t('settings.output.postprocess.mp42RequiresMp4') }}
        </p>

        <p v-else-if="showKeepOriginalStreamsConflict" class="alert alert-error mt-2 alert-soft">
          {{ t('settings.output.postprocess.keepOriginalStreamsConflict') }}
        </p>

        <p v-if="showReencodeWarning" class="alert alert-warning mt-2 alert-soft">
          {{ t('settings.output.postprocess.reencodeWarning') }}
        </p>

        <slot name="video-extra" />
      </template>

      <template #audio>
        <h2 class="sr-only">
          {{ t('settings.output.tabs.audio.screenreader') }}
        </h2>

        <base-select
            :label="t('settings.output.tabs.audio.format.label')"
            :hint="t('settings.output.tabs.audio.format.hint')"
            :options="AudioFormat"
            :locale-key="'settings.output.tabs.audio.format.options'"
            v-model="outputState.audio.format"
            class="mb-4"
        />

        <div class="mb-4 flex flex-col gap-1">
          <label class="font-semibold" :for="fieldId('keep-original-streams-audio')">
            {{ t('settings.output.keepOriginalStreams.label') }}
          </label>
          <input
              :id="fieldId('keep-original-streams-audio')"
              type="checkbox"
              v-model="keepOriginalStreamsAudio"
              class="toggle toggle-primary my-1"
          />
          <p class="label">
            {{ t('settings.output.keepOriginalStreams.hint') }}
          </p>
        </div>

        <base-select
            :label="t('settings.output.tabs.audio.postprocess.label')"
            :hint="t('settings.output.postprocess.hint')"
            :options="AudioPostprocessPreset"
            :locale-key="'settings.output.tabs.audio.postprocess.options'"
            v-model="outputState.audio.postprocessPreset"
        />

        <template v-if="showAudioPostprocessArgs">
          <label class="mb-2 mt-4 font-semibold" :for="fieldId('audio-postprocess-args')">
            {{ t('settings.output.postprocessArgs.label') }}
          </label>
          <textarea
              :id="fieldId('audio-postprocess-args')"
              v-model="outputState.audio.postprocessArgs"
              class="textarea rounded-xl w-full"
              rows="3"
              :placeholder="t('settings.output.postprocessArgs.hint')"
          />
        </template>

        <slot name="audio-extra" />
      </template>

      <template #uniqueness>
        <h2 class="sr-only">
          {{ t('settings.output.tabs.uniqueness.screenreader') }}
        </h2>

        <div class="mb-4 flex flex-col gap-1">
          <label class="font-semibold" :for="fieldId('uniqueness-enabled')">
            {{ t('settings.output.tabs.uniqueness.enabled.label') }}
          </label>
          <input
              :id="fieldId('uniqueness-enabled')"
              type="checkbox"
              v-model="outputState.video.uniqueness.enabled"
              class="toggle toggle-primary my-1"
          />
          <p class="label">
            {{ t('settings.output.tabs.uniqueness.enabled.hint') }}
          </p>
        </div>

        <template v-if="outputState.video.uniqueness.enabled">
          <div class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-flip')">
              {{ t('settings.output.tabs.uniqueness.flip.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-flip')"
                type="checkbox"
                v-model="outputState.video.uniqueness.flipHorizontal"
                class="toggle toggle-primary my-1"
            />
          </div>

          <div class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-crop')">
              {{ t('settings.output.tabs.uniqueness.crop.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-crop')"
                type="checkbox"
                v-model="outputState.video.uniqueness.autoCrop"
                class="toggle toggle-primary my-1"
            />
          </div>

          <div class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-ai-captions')">
              {{ t('settings.output.tabs.uniqueness.aiCaptions.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-ai-captions')"
                type="checkbox"
                v-model="outputState.video.uniqueness.aiCaptions"
                class="toggle toggle-primary my-1"
            />
            <p class="label">
              {{ t('settings.output.tabs.uniqueness.aiCaptions.hint') }}
            </p>
          </div>

          <div v-if="outputState.video.uniqueness.aiCaptions" class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-caption-provider')">
              {{ t('settings.output.tabs.uniqueness.captionProvider.label') }}
            </label>
            <select
                :id="fieldId('uniqueness-caption-provider')"
                v-model="outputState.video.uniqueness.captionProvider"
                class="select select-bordered rounded-xl w-full"
            >
              <option value="openai">OpenAI (Whisper)</option>
              <option value="groq">Groq (Whisper - Fast/Free)</option>
            </select>
          </div>

          <div class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-ai-metadata')">
              {{ t('settings.output.tabs.uniqueness.aiMetadata.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-ai-metadata')"
                type="checkbox"
                v-model="outputState.video.uniqueness.aiMetadata"
                class="toggle toggle-primary my-1"
            />
            <p class="label">
              {{ t('settings.output.tabs.uniqueness.aiMetadata.hint') }}
            </p>
          </div>

          <div class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-ai-shorts')">
              {{ t('settings.output.tabs.uniqueness.aiShortsMode.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-ai-shorts')"
                type="checkbox"
                v-model="outputState.video.uniqueness.aiShortsMode"
                class="toggle toggle-primary my-1"
            />
            <p class="label">
              {{ t('settings.output.tabs.uniqueness.aiShortsMode.hint') }}
            </p>
          </div>

          <div v-if="outputState.video.uniqueness.aiShortsMode" class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-num-shorts')">
              {{ t('settings.output.tabs.uniqueness.numShorts.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-num-shorts')"
                type="number"
                v-model="outputState.video.uniqueness.numShorts"
                min="1"
                max="10"
                class="input input-bordered rounded-xl w-full"
            />
          </div>

          <div class="mb-4 flex flex-col gap-1">
            <label class="font-semibold" :for="fieldId('uniqueness-dubbing')">
              {{ t('settings.output.tabs.uniqueness.dubbing.label') }}
            </label>
            <input
                :id="fieldId('uniqueness-dubbing')"
                type="checkbox"
                v-model="outputState.video.uniqueness.dubbingEnabled"
                class="toggle toggle-primary my-1"
            />
            <p class="label">
              {{ t('settings.output.tabs.uniqueness.dubbing.hint') }}
            </p>
          </div>

          <template v-if="outputState.video.uniqueness.dubbingEnabled">
            <div class="mb-4 flex flex-col gap-1">
              <label class="font-semibold" :for="fieldId('uniqueness-target-language')">
                {{ t('settings.output.tabs.uniqueness.targetLanguage.label') }}
              </label>
              <select
                  :id="fieldId('uniqueness-target-language')"
                  v-model="outputState.video.uniqueness.targetLanguage"
                  class="select select-bordered rounded-xl w-full"
              >
                <option value="en-US">English (United States)</option>
                <option value="en-GB">English (United Kingdom)</option>
                <option value="km-KH">Khmer (Cambodia)</option>
                <option value="zh-CN">Chinese (Mandarin)</option>
                <option value="es-ES">Spanish</option>
                <option value="fr-FR">French</option>
                <option value="de-DE">German</option>
                <option value="ja-JP">Japanese</option>
                <option value="ko-KR">Korean</option>
                <option value="vi-VN">Vietnamese</option>
                <option value="th-TH">Thai</option>
                <option value="id-ID">Indonesian</option>
              </select>
            </div>

            <div class="mb-4 flex flex-col gap-1">
              <label class="font-semibold" :for="fieldId('uniqueness-voice-id')">
                {{ t('settings.output.tabs.uniqueness.voiceCloneId.label') }}
              </label>
              <input
                  :id="fieldId('uniqueness-voice-id')"
                  type="text"
                  v-model="outputState.video.uniqueness.voiceCloneId"
                  class="input input-bordered rounded-xl w-full"
                  :placeholder="t('settings.output.tabs.uniqueness.voiceCloneId.hint')"
              />
            </div>
          </template>

          <label class="mb-2 mt-4 font-semibold" :for="fieldId('uniqueness-watermark')">
            {{ t('settings.output.tabs.uniqueness.watermark.label') }}
          </label>
          <input
              :id="fieldId('uniqueness-watermark')"
              type="text"
              v-model="outputState.video.uniqueness.watermarkText"
              class="input input-bordered rounded-xl w-full"
              :placeholder="t('settings.output.tabs.uniqueness.watermark.hint')"
          />
        </template>
      </template>
    </base-tabbed-pane>

    <div class="mt-4 mb-4 flex flex-col gap-1">
      <label class="font-semibold" :for="fieldId('add-thumbnail')">
        {{ t('settings.output.addThumbnail.label') }}
      </label>
      <input
          :id="fieldId('add-thumbnail')"
          type="checkbox"
          v-model="outputState.addThumbnail"
          class="toggle toggle-primary my-1"
      />
      <p class="label">
        {{ t('settings.output.addThumbnail.hint') }}
      </p>
    </div>

    <div class="mb-4 flex flex-col gap-1">
      <label class="font-semibold" :for="fieldId('add-thumbnail')">
        {{ t('settings.output.saveThumbnail.label') }}
      </label>
      <input
          :id="fieldId('save-thumbnail')"
          type="checkbox"
          v-model="outputState.saveThumbnail"
          class="toggle toggle-primary my-1"
      />
      <p class="label">
        {{ t('settings.output.saveThumbnail.hint') }}
      </p>
    </div>

    <div class="mb-4 flex flex-col gap-1">
      <label class="font-semibold" :for="fieldId('add-metadata')">
        {{ t('settings.output.addMetadata.label') }}
      </label>
      <input
          :id="fieldId('add-metadata')"
          type="checkbox"
          v-model="outputState.addMetadata"
          class="toggle toggle-primary my-1"
      />
      <p class="label">
        {{ t('settings.output.addMetadata.hint') }}
      </p>
    </div>

    <div v-if="showPreciseCuts" class="mb-4 flex flex-col gap-1">
      <label class="font-semibold" :for="fieldId('precise-cuts')">
        {{ t('settings.output.preciseCuts.label') }}
      </label>
      <input
          :id="fieldId('precise-cuts')"
          type="checkbox"
          v-model="outputState.preciseCuts"
          class="toggle toggle-primary my-1"
      />
      <p class="label whitespace-pre-line">
        {{ t('settings.output.preciseCuts.hint') }}
      </p>
    </div>

    <slot name="after-common" />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import BaseSelect from '../base/BaseSelect.vue';
import BaseTabbedPane from '../base/BaseTabbedPane.vue';
import type { OutputSettings } from '../../tauri/types/config.ts';
import {
  AudioFormat,
  AudioPostprocessPreset,
  VideoContainer,
  VideoPostprocessMode,
  VideoPostprocessPreset,
} from '../../tauri/types/media.ts';
import { useOutputSettingsEditor } from '../../composables/useOutputSettingsEditor.ts';

const outputState = defineModel<OutputSettings>({ required: true });

const { idPrefix } = defineProps({
  idPrefix: {
    type: String,
    required: false,
    default: '',
  },
  showPreciseCuts: {
    type: Boolean,
    required: false,
    default: true,
  },
});

const { t } = useI18n();
const {
  activeTab,
  tabs,
  keepOriginalStreamsVideo,
  keepOriginalStreamsAudio,
  postprocessHint,
  showVideoPostprocessArgs,
  showVideoPostprocessMode,
  showAudioPostprocessArgs,
  showReencodeWarning,
  showKeepOriginalStreamsConflict,
  showMp42ContainerConflict,
} = useOutputSettingsEditor(outputState);

const tabIdPrefix = computed(() => idPrefix || 'output');

const fieldId = (name: string) => idPrefix ? `${idPrefix}-${name}` : name;
</script>
