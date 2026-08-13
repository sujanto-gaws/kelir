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
 */
withDefaults(
  defineProps<{
    title: string
    description: string
    confirmLabel?: string
    /** The backend's own words when the action was refused. */
    error?: string
    pending?: boolean
  }>(),
  {
    confirmLabel: 'Confirm',
    error: '',
    pending: false,
  },
)

const emit = defineEmits<{ confirm: [] }>()

const open = defineModel<boolean>('open', { default: false })
</script>

<template>
  <Dialog v-model:open="open" :title="title">
    <p class="text-sm text-muted-foreground">{{ description }}</p>

    <Alert v-if="error" variant="destructive" class="mt-4">{{ error }}</Alert>

    <template #footer>
      <Button variant="outline" :disabled="pending" @click="open = false">Cancel</Button>
      <Button variant="destructive" :loading="pending" @click="emit('confirm')">
        {{ confirmLabel }}
      </Button>
    </template>
  </Dialog>
</template>
