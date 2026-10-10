<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from 'vue'
import { X } from '@lucide/vue'

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

/** What held focus when the dialog opened, to be handed it back on close. */
let opener: HTMLElement | null = null

/**
 * Hands focus back to what opened the dialog (WAI-ARIA dialog pattern; added
 * 2026-10-10, PR #715's campaign): without it a keyboard user is left on
 * `<body>` and restarts from the top of the page.
 *
 * **Only when nothing else has taken focus since**: a dialog that closes as
 * another opens leaves focus with the one opening. And only to an element
 * still in the document, since what opened it may be gone, as *Deprecate* is
 * once its revision is deprecated.
 */
function restoreFocus(): void {
  const target = opener

  opener = null

  const active = document.activeElement

  if (target?.isConnected && (active === null || active === document.body)) {
    target.focus()
  }
}

// Move focus into the panel so Escape and the tab order start inside the
// dialog, and hand it back on close. No focus trap: tabbing past the last
// control escapes the panel.
watch(
  open,
  async (isOpen) => {
    if (!isOpen) {
      await nextTick()
      restoreFocus()

      return
    }

    const active = document.activeElement

    opener = active instanceof HTMLElement && active !== document.body ? active : null

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
