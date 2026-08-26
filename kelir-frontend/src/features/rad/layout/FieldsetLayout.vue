<script setup lang="ts">
import { computed } from 'vue'

import type { JfssComponent, JfssLayoutComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssLayoutComponent }>()

/**
 * A grouped set of related fields (JFSS §4.3).
 *
 * A real `<fieldset>` and `<legend>` rather than a bordered div with a heading:
 * the pair is what tells a screen reader that these controls belong together,
 * and it is the semantic a form author reaches for `fieldset` to get.
 */
const children = computed<JfssComponent[]>(() => props.component.components ?? [])
</script>

<template>
  <fieldset class="space-y-4 rounded-md border border-border p-4">
    <legend v-if="component.title" class="px-1 text-base font-semibold">
      {{ component.title }}
    </legend>

    <slot name="group" :components="children" />
  </fieldset>
</template>
