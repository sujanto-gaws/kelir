<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { getLinkedEntity } from '@/api/documents'
import { ApiError } from '@/api/error'
import { Badge } from '@/components/ui/badge'
import {
  DOCUMENT_PRIORITY_LABELS,
  DOCUMENT_STATUS_LABELS,
  ENTITY_TYPE_LABELS,
  type Document,
  type ResolvedEntity,
} from '@/types/document'

const props = defineProps<{ document: Document }>()

/**
 * The things a person opening a document is looking for, without opening a tab
 * (#172 AC2): its status, its number and what it concerns.
 *
 * # The linked entity is resolved by a second call, and that is the design
 *
 * Reading a document hands back `entityType` and `entityId` and nothing about
 * the record they name. Resolving them requires the entity's *own* read
 * permission, enforced by the backend calling the master-data service rather
 * than checking a string of its own — which is #161's answer to the same
 * question, and the reason a document cannot become a way to read master data
 * the caller could not read directly.
 *
 * So a caller who may read documents and not suppliers sees **the identifier
 * and a sentence saying the name could not be loaded**, rather than a blank
 * where a name should be. An empty name would be a false statement about the
 * supplier; this one is true about the reader.
 */
const entity = ref<ResolvedEntity | null>(null)
const entityRefused = ref(false)

watch(
  () => [props.document.id, props.document.entityId] as const,
  async ([id, entityId]) => {
    entity.value = null
    entityRefused.value = false

    if (!entityId) {
      return
    }

    try {
      entity.value = await getLinkedEntity(id)
    } catch (error) {
      // A 403 (the caller may not read this record) and a 404 (the record has
      // been retired) are deliberately not distinguished on screen. Both mean
      // "this document concerns something you are not being shown", and telling
      // a caller which would tell them whether a record they may not read
      // exists.
      entityRefused.value = error instanceof ApiError
    }
  },
  { immediate: true },
)

const statusVariant = computed(() => {
  const status = props.document.status

  if (status === 'REJECTED' || status === 'CANCELLED') {
    return 'destructive' as const
  }

  if (status === 'COMPLETED' || status === 'APPROVED') {
    return 'default' as const
  }

  return 'secondary' as const
})
</script>

<template>
  <header class="space-y-3 border-b border-border pb-4">
    <div class="flex flex-wrap items-center gap-3">
      <h1 class="text-xl font-semibold" data-testid="document-title">{{ document.title }}</h1>
      <Badge :variant="statusVariant" data-testid="document-status">
        {{ DOCUMENT_STATUS_LABELS[document.status] }}
      </Badge>
    </div>

    <dl class="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2 lg:grid-cols-4">
      <div>
        <dt class="text-muted-foreground">Number</dt>
        <!-- A draft has no number and will not have one until it is submitted.
             Saying so beats an empty cell, which reads as a value that failed
             to load. -->
        <dd v-if="document.documentNumber" class="font-medium" data-testid="document-number">
          {{ document.documentNumber }}
        </dd>
        <dd v-else class="text-muted-foreground" data-testid="document-number">
          Assigned when this is submitted
        </dd>
      </div>

      <div>
        <dt class="text-muted-foreground">Reference</dt>
        <dd class="font-medium" data-testid="document-ref">{{ document.documentRef }}</dd>
      </div>

      <div>
        <dt class="text-muted-foreground">Priority</dt>
        <dd class="font-medium">{{ DOCUMENT_PRIORITY_LABELS[document.priority] }}</dd>
      </div>

      <div v-if="document.entityType && document.entityId">
        <dt class="text-muted-foreground">{{ ENTITY_TYPE_LABELS[document.entityType] }}</dt>
        <dd v-if="entity" class="font-medium" data-testid="document-entity">
          {{ entity.name }} <span class="text-muted-foreground">({{ entity.code }})</span>
        </dd>
        <dd v-else-if="entityRefused" class="text-muted-foreground" data-testid="document-entity">
          {{ document.entityId }} — not available to you
        </dd>
        <dd v-else class="text-muted-foreground" data-testid="document-entity">Loading…</dd>
      </div>
    </dl>
  </header>
</template>
