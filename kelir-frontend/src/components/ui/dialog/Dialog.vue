<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from 'vue'
import { X } from 'lucide-vue-next'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

const props = withDefaults(
  defineProps<{
    title: string
    description?: string
    class?: string
  }>(),
  {
    description: undefined,
    class: undefined,
  },
)

const open = defineModel<boolean>('open', { default: false })

const titleId = useId()
const descriptionId = useId()

const panel = ref<HTMLElement | null>(null)

const classes = computed(() =>
  cn(
    'relative w-full max-w-lg max-h-[85vh] overflow-y-auto rounded-lg border border-border bg-card p-6 shadow-lg',
    props.class,
  ),
)

function close(): void {
  open.value = false
}

// Move focus into the panel so Escape and the tab order start inside the dialog.
// No focus trap: tabbing past the last control escapes the panel.
watch(
  open,
  async (isOpen) => {
    if (!isOpen) {
      return
    }

    await nextTick()
    panel.value?.focus()
  },
  { immediate: true },
)
</script>

<template>
  <!-- No Teleport: it puts the panel outside the mounted tree, which makes the
       dialog untestable with the default @vue/test-utils mount. -->
  <div
    v-if="open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
    @click.self="close"
    @keydown.esc="close"
  >
    <div
      ref="panel"
      :class="classes"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="titleId"
      :aria-describedby="description === undefined ? undefined : descriptionId"
      tabindex="-1"
    >
      <div class="flex items-start justify-between gap-4">
        <div class="space-y-1">
          <h2 :id="titleId" class="text-lg font-medium">{{ title }}</h2>
          <p
            v-if="description !== undefined"
            :id="descriptionId"
            class="text-sm text-muted-foreground"
          >
            {{ description }}
          </p>
        </div>
        <Button variant="ghost" size="icon" aria-label="Close" @click="close">
          <X class="size-4" aria-hidden="true" />
        </Button>
      </div>

      <div class="mt-4 text-sm">
        <slot />
      </div>

      <div v-if="$slots.footer" class="mt-6 flex justify-end gap-2">
        <slot name="footer" />
      </div>
    </div>
  </div>
</template>
