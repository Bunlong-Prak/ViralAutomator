<template>
  <SettingsAi v-model="settings" />
</template>

<script setup lang="ts">
import { Settings } from '../../../tauri/types/config.ts';
import SettingsAi from '../../../components/settings/SettingsAi.vue';

const settings = defineModel<Settings>({ required: true });
</script>
