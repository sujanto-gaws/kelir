<script setup lang="ts">
import { computed } from 'vue'

import { Alert } from '@/components/ui/alert'
import { unplacedErrors } from '@/composables/useFormErrors'

/**
 * The server's field errors a hand-built form has nowhere else to show
 * (coding standard §3.4, #576).
 *
 * A form passes what `useFormErrors` holds and the paths it shows a message
 * for **as drawn now**: a field its current mode leaves out is not placed.
 * Whatever is left is listed here as `path: message`, so a refused save always
 * says why. Nothing is drawn when every detail has a place, and a detail is
 * never both under its input and here.
 *
 * It is an `Alert`, so it is announced when it appears, as the form's own
 * message is (NFR-USE-005). A `data-testid` or a class falls through to it.
 */
const props = defineProps<{
  /** `useFormErrors().fieldErrors`: the server's messages by path. */
  fieldErrors: Record<string, string>
  /** The paths this form shows a message for, as it is drawn now. */
  placed: readonly string[]
}>()

const unplaced = computed(() => unplacedErrors(props.fieldErrors, props.placed))
</script>

<template>
  <Alert v-if="unplaced.length > 0" variant="destructive">
    <ul class="list-disc pl-4">
      <li v-for="item in unplaced" :key="item.path">{{ item.path }}: {{ item.message }}</li>
    </ul>
  </Alert>
</template>
