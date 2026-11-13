<template>
  <base-fieldset
      :legend="t('settings.ai.legend')"
      :label="t('settings.ai.legendLabel')"
  >
    <div class="flex flex-col gap-4">
      <div class="flex flex-col gap-1">
        <label class="font-semibold" for="openai-api-key">
          {{ t('settings.ai.openai.label') }}
        </label>
        <base-secret-input
            id="openai-api-key"
            v-model="aiState.openaiApiKey"
            :placeholder="t('settings.ai.openai.hint')"
            :password="true"
        />
      </div>

      <div class="flex flex-col gap-1">
        <label class="font-semibold" for="groq-api-key">
          {{ t('settings.ai.groq.label') }}
        </label>
        <base-secret-input
            id="groq-api-key"
            v-model="aiState.groqApiKey"
            :placeholder="t('settings.ai.groq.hint')"
            :password="true"
        />
      </div>

      <div class="flex flex-col gap-1">
        <label class="font-semibold" for="deepseek-api-key">
          {{ t('settings.ai.deepseek.label') }}
        </label>
        <base-secret-input
            id="deepseek-api-key"
            v-model="aiState.deepseekApiKey"
            :placeholder="t('settings.ai.deepseek.hint')"
            :password="true"
        />
      </div>

      <div class="flex flex-col gap-1">
        <label class="font-semibold" for="gemini-api-key">
          {{ t('settings.ai.gemini.label') }}
        </label>
        <base-secret-input
            id="gemini-api-key"
            v-model="aiState.geminiApiKey"
            :placeholder="t('settings.ai.gemini.hint')"
            :password="true"
        />
      </div>

      <div class="flex flex-col gap-1">
        <label class="font-semibold" for="caption-style">
          {{ t('settings.ai.captionStyle.label') }}
        </label>
        <input
            id="caption-style"
            type="text"
            v-model="aiState.captionStyle"
            class="input input-bordered rounded-xl w-full"
            :placeholder="t('settings.ai.captionStyle.hint')"
        />
        <p class="label">
          {{ t('settings.ai.captionStyle.hint') }}
        </p>
      </div>

      <div class="flex flex-col gap-1">
        <label class="font-semibold" for="voxcpm-api-url">
          {{ t('settings.ai.voxcpm.label') }}
        </label>
        <input
            id="voxcpm-api-url"
            type="text"
            v-model="aiState.voxcpmApiUrl"
            class="input input-bordered rounded-xl w-full"
            :placeholder="t('settings.ai.voxcpm.hint')"
        />
        <p class="label">
          {{ t('settings.ai.voxcpm.hint') }}
        </p>
      </div>
    </div>
  </base-fieldset>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import BaseFieldset from '../base/BaseFieldset.vue';
import BaseSecretInput from '../base/BaseSecretInput.vue';
import { Settings } from '../../tauri/types/config.ts';

const { t } = useI18n();
const settings = defineModel<Settings>({ required: true });

const aiState = computed({
  get: () => settings.value.ai,
  set: (value) => {
    settings.value.ai = value;
  },
});
</script>
