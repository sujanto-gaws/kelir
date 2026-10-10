<script setup lang="ts">
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'

/**
 * Confirmation for a destructive action.
 *
 * The parent owns the call and passes its state back down, rather than this
 * dialog running the request itself: a refusal — the backend's 409 for a system
 * role, its 400 for deactivating yourself — has to be readable *here*, next to
 * the button that caused it, instead of behind a toast the user has already
 * dismissed. The dialog therefore stays open on failure and closes only when
 * the parent says so.
 *
 * **The default slot adds what one sentence cannot say**, under the
 * description: the workflow editor's *Deprecate* lists the document types still
 * bound to the revision there (#713). `confirmDisabled` holds the confirm button
 * while that list is still being read, and leaves *Cancel* free.
 */
withDefaults(
  defineProps<{
    title: string
    description: string
    confirmLabel?: string
    /** The backend's own words when the action was refused. */
    error?: string
    pending?: boolean
    /** Hold the confirm button, as while what the dialog warns about is still being read. */
    confirmDisabled?: boolean
  }>(),
  {
    confirmLabel: 'Confirm',
    error: '',
    pending: false,
    confirmDisabled: false,
  },
)

const emit = defineEmits<{ confirm: [] }>()

const open = defineModel<boolean>('open', { default: false })
</script>

<template>
  <Dialog v-model:open="open" :title="title">
    <p class="text-sm text-muted-foreground">{{ description }}</p>

    <div v-if="$slots.default" class="mt-4">
      <slot />
    </div>

    <Alert v-if="error" variant="destructive" class="mt-4">{{ error }}</Alert>

    <template #footer>
      <Button variant="outline" :disabled="pending" @click="open = false">Cancel</Button>
      <Button
        variant="destructive"
        :loading="pending"
        :disabled="confirmDisabled"
        data-testid="confirm-action"
        @click="emit('confirm')"
      >
        {{ confirmLabel }}
      </Button>
    </template>
  </Dialog>
</template>
