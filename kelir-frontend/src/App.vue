<script setup lang="ts">
import { onMounted } from 'vue'
import { RouterView } from 'vue-router'

import { useAppStore } from '@/stores/app'

const appStore = useAppStore()

// Follow the operating system's preference on first load. A user-level override
// is a settings-module concern (Phase 9), not a shell one.
onMounted(() => {
  appStore.setTheme(window.matchMedia('(prefers-color-scheme: dark)').matches)
})
</script>

<template>
  <RouterView />
</template>
