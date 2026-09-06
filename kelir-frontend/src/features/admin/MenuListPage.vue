<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { toApiError } from '@/api/client'
import { deleteMenu, listMenus } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useAuthStore } from '@/stores/auth'
import type { MenuEntry } from '@/types/rad'

import ConfirmDialog from './ConfirmDialog.vue'
import MenuFormDialog from './MenuFormDialog.vue'

/**
 * The configured navigation (FR-RAD-004; #341 AC2).
 *
 * **`rad_menus` had no surface at all.** The table has been in the schema since
 * Sprint 7 with no domain type, no endpoint and no screen; this is the screen,
 * and what it authors joins the built-in destinations in the sidebar rather
 * than replacing them.
 *
 * **The tree is shown by indentation rather than by a widget.** An entry's
 * depth is what a person needs to see to choose its parent, and a collapsing
 * tree control would hide exactly the rows they came here to compare. Depth is
 * computed here and bounded, so a cycle that reached the table another way
 * renders a flat list rather than hanging the page — the API refuses to create
 * one, and a renderer that trusted it would be a renderer with no answer when
 * the refusal was wrong.
 */
const auth = useAuthStore()

const canCreate = computed(() => auth.can('rad:menu:create'))
const canUpdate = computed(() => auth.can('rad:menu:update'))
const canDelete = computed(() => auth.can('rad:menu:delete'))

const entries = ref<MenuEntry[]>([])
const isLoading = ref(false)
const error = ref('')

const isFormOpen = ref(false)
const editing = ref<MenuEntry | null>(null)

const confirming = ref<MenuEntry | null>(null)
const isConfirmOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

/** How deep the list will indent before it stops trusting the tree. */
const MAX_DEPTH = 8

/**
 * The entries in tree order, each with the depth it renders at.
 *
 * Walked from the roots down rather than sorted by depth, so a child always
 * follows its parent. An entry whose parent is missing from the page is
 * rendered as a root rather than dropped: it exists, and a builder that hid it
 * would be hiding the row somebody needs to fix.
 */
const rows = computed(() => {
  const byParent = new Map<string | null, MenuEntry[]>()

  for (const entry of entries.value) {
    const key = entry.parentMenuId
    const known = key !== null && entries.value.some((other) => other.id === key)
    const bucket = known ? key : null

    byParent.set(bucket, [...(byParent.get(bucket) ?? []), entry])
  }

  const ordered: { entry: MenuEntry; depth: number }[] = []

  const walk = (parent: string | null, depth: number): void => {
    if (depth > MAX_DEPTH) {
      return
    }

    for (const entry of byParent.get(parent) ?? []) {
      ordered.push({ entry, depth })
      walk(entry.id, depth + 1)
    }
  }

  walk(null, 0)

  return ordered
})

async function load(): Promise<void> {
  isLoading.value = true
  error.value = ''

  try {
    entries.value = (await listMenus()).items
  } catch (failure) {
    error.value = toApiError(failure).message
  } finally {
    isLoading.value = false
  }
}

function startCreating(): void {
  editing.value = null
  isFormOpen.value = true
}

function startEditing(entry: MenuEntry): void {
  editing.value = entry
  isFormOpen.value = true
}

async function afterSave(): Promise<void> {
  isFormOpen.value = false
  editing.value = null
  await load()
}

function confirmDelete(entry: MenuEntry): void {
  confirming.value = entry
  deleteError.value = ''
  isConfirmOpen.value = true
}

async function remove(): Promise<void> {
  const target = confirming.value

  if (!target) {
    return
  }

  isDeleting.value = true
  deleteError.value = ''

  try {
    await deleteMenu(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await load()
  } catch (failure) {
    deleteError.value = toApiError(failure).message
  } finally {
    isDeleting.value = false
  }
}

onMounted(() => {
  void load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Navigation</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          What this tenant adds to the sidebar. The built-in destinations are always there.
        </p>
      </div>

      <Button v-if="canCreate" data-testid="new-menu" @click="startCreating">New entry</Button>
    </div>

    <Alert v-if="error" variant="destructive" data-testid="menus-error">{{ error }}</Alert>

    <Table data-testid="menus">
      <TableHeader>
        <TableRow>
          <TableHead>Label</TableHead>
          <TableHead>Route</TableHead>
          <TableHead>Hidden unless</TableHead>
          <TableHead>Order</TableHead>
          <TableHead>Shown</TableHead>
          <TableHead class="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>

      <TableBody>
        <TableRow
          v-for="row in rows"
          :key="row.entry.id"
          :data-testid="`menu-${row.entry.menuKey}`"
        >
          <TableCell class="font-medium">
            <span :style="{ paddingLeft: `${row.depth * 1.25}rem` }">{{ row.entry.label }}</span>
          </TableCell>
          <TableCell>
            <span v-if="row.entry.routePath" class="text-sm">{{ row.entry.routePath }}</span>
            <span v-else class="text-sm text-muted-foreground">Heading</span>
          </TableCell>
          <TableCell class="text-sm">{{ row.entry.requiredPermission ?? 'Everyone' }}</TableCell>
          <TableCell>{{ row.entry.sortOrder }}</TableCell>
          <TableCell>
            <Badge :variant="row.entry.isEnabled ? 'default' : 'secondary'">
              {{ row.entry.isEnabled ? 'Yes' : 'No' }}
            </Badge>
          </TableCell>
          <TableCell class="space-x-2 text-right">
            <Button
              v-if="canUpdate"
              size="sm"
              variant="secondary"
              :data-testid="`edit-menu-${row.entry.menuKey}`"
              @click="startEditing(row.entry)"
            >
              Edit
            </Button>
            <Button
              v-if="canDelete"
              size="sm"
              variant="destructive"
              :data-testid="`delete-menu-${row.entry.menuKey}`"
              @click="confirmDelete(row.entry)"
            >
              Delete
            </Button>
          </TableCell>
        </TableRow>

        <TableRow v-if="!isLoading && rows.length === 0">
          <TableCell colspan="6" class="text-center text-sm text-muted-foreground">
            This tenant has added nothing to the navigation yet.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <MenuFormDialog
      :open="isFormOpen"
      :editing="editing"
      :entries="entries"
      @close="isFormOpen = false"
      @saved="afterSave"
    />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Remove navigation entry"
      :description="`${confirming?.label ?? 'This entry'} will be removed. Anything under it moves up a level.`"
      confirm-label="Remove"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="remove()"
    />
  </section>
</template>
