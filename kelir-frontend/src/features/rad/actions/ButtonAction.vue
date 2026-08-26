<script setup lang="ts">
import { computed } from 'vue'

import { Button } from '@/components/ui/button'
import type { ButtonVariant } from '@/components/ui/button'
import type { JfssActionComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssActionComponent }>()
const emit = defineEmits<{ (e: 'action', action: string): void }>()

/**
 * `button` — an action trigger (JFSS §4.5).
 *
 * **It emits the definition's `action` string and decides nothing.** What
 * `submit` means is #164's, and what `reset` means is the form's; a button that
 * knew would be a button that could disagree with the form it sits in. `action`
 * is an open vocabulary like `type` is, so an unrecognised one reaches the form
 * root and is handled there rather than swallowed here.
 *
 * `type="button"` on every one of them, including submit: the form's own submit
 * handler is what runs, so a native submission that reloads the page is a
 * failure mode this simply does not have.
 */
const THEMES: Record<string, ButtonVariant> = {
  primary: 'default',
  secondary: 'secondary',
  danger: 'destructive',
  destructive: 'destructive',
  link: 'link',
}

const variant = computed<ButtonVariant>(
  () => THEMES[props.component.theme ?? ''] ?? (props.component.action === 'submit' ? 'default' : 'secondary'),
)
</script>

<template>
  <Button type="button" :variant="variant" @click="emit('action', component.action)">
    {{ component.label }}
  </Button>
</template>
