<script setup lang="ts">
import { computed } from 'vue'

import { Alert } from '@/components/ui/alert'
import ConfirmDialog from '@/features/admin/ConfirmDialog.vue'

import type { WorkflowDeprecation } from './useWorkflowDeprecation'

/**
 * The warning before a revision is deprecated (#713, **D-101** B).
 *
 * **It names the document types still bound to the revision**, because each
 * one's submissions may be refused from the moment it is deprecated until
 * somebody binds the type to a published revision. *May*, since the list holds
 * every live binding whatever its dates: a type whose document another binding
 * in force routes first, or whose binding has lapsed, is not blocked (corrected
 * 2026-10-10: this said *are refused*). Where they could not be read it says so,
 * and says what deprecating may do to any there are.
 */
const props = defineProps<{ deprecation: WorkflowDeprecation }>()

const emit = defineEmits<{ confirm: [] }>()

const target = computed(() => props.deprecation.target.value)
const bound = computed(() => props.deprecation.bound.value)

const title = computed(() =>
  target.value
    ? `Deprecate revision ${target.value.version} of ${target.value.workflowKey}?`
    : 'Deprecate this revision?',
)

const isOpen = computed({
  get: () => props.deprecation.isOpen.value,
  // The dialog only ever closes itself: Cancel, the close button, Escape.
  set: (value: boolean) => {
    if (!value) {
      props.deprecation.cancel()
    }
  },
})

const unnamed = computed(() =>
  bound.value.kind === 'listed' ? bound.value.total - bound.value.types.length : 0,
)
</script>

<template>
  <ConfirmDialog
    v-model:open="isOpen"
    :title="title"
    description="New documents stop routing to this revision, and it can no longer be bound to a document type. Approvals already running on it carry on. A deprecated revision is not made active again."
    confirm-label="Deprecate"
    :confirm-disabled="bound.kind === 'checking'"
    @confirm="emit('confirm')"
  >
    <p
      v-if="bound.kind === 'checking'"
      class="text-sm text-muted-foreground"
      data-testid="bound-types-checking"
    >
      Checking which document types are still bound to this revision…
    </p>

    <p
      v-else-if="bound.kind === 'listed' && bound.total === 0"
      class="text-sm"
      data-testid="bound-types-none"
    >
      No document type is bound to this revision.
    </p>

    <Alert v-else-if="bound.kind === 'listed'" variant="destructive" data-testid="bound-types">
      <p>
        {{ bound.total === 1 ? 'This document type is' : 'These document types are' }} still bound
        to this revision. Deprecating it may block their submissions until each is bound to a
        published revision, unless another binding routes them first or theirs has lapsed:
      </p>
      <ul class="mt-2 list-disc space-y-1 pl-5">
        <li v-for="type in bound.types" :key="type.id" data-testid="bound-type">
          {{ type.name }} <span class="font-mono text-xs">({{ type.typeCode }})</span>
        </li>
      </ul>
      <p v-if="unnamed > 0" class="mt-2" data-testid="bound-types-more">
        and {{ unnamed }} more, {{ bound.total }} in all.
      </p>
    </Alert>

    <Alert v-else variant="destructive" data-testid="bound-types-unchecked">
      {{ bound.reason }} Deprecating may block submissions for any document type still bound to it,
      until that type is bound to a published revision.
    </Alert>
  </ConfirmDialog>
</template>
