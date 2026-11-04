<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { toApiError } from '@/api/client'
import { createMenu, updateMenu } from '@/api/rad'
import FormUnplacedErrors from '@/components/FormUnplacedErrors.vue'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { useFormErrors } from '@/composables/useFormErrors'
import type { MenuEntry } from '@/types/rad'

/**
 * Creating and editing a navigation entry (FR-RAD-004; #341 AC2).
 *
 * **A parent is chosen, not typed.** The API refuses a parent that would close
 * a loop — that is [#191](https://github.com/sujanto-gaws/kelir/issues/191)'s
 * ancestor walk — and it also refuses one that is not this tenant's. Both
 * refusals are shown by the field they are about rather than as a banner,
 * because both are answers to *which parent*.
 *
 * **The entry itself is not filtered out of its own parent list.** Offering it
 * would let somebody pick the one choice the server certainly refuses; the
 * descendants below it are *not* filtered, because working out which they are
 * is the walk the server does, and a client that duplicated it would be a
 * second copy of the rule that could drift.
 */
const props = defineProps<{ open: boolean; editing: MenuEntry | null; entries: MenuEntry[] }>()
const emit = defineEmits<{ (event: 'close'): void; (event: 'saved'): void }>()

const menuKey = ref('')
const label = ref('')
const icon = ref('')
const routePath = ref('')
const requiredPermission = ref('')
const parentId = ref('')
const sortOrder = ref('0')
const isEnabled = ref(true)

const isSaving = ref(false)
const { fieldErrors, formError, report, reset } = useFormErrors()

/**
 * The paths a message is shown under (#641, #672). Each is drawn in both modes:
 * on an edit the key is disabled, not hidden, and its slot stays. `icon` and
 * `requiredPermission` are bounded by the backend (`TOO_LONG`), so each has a
 * slot of its own. Any other path is listed above the form.
 */
const PLACED = ['menuKey', 'label', 'routePath', 'parentMenuId', 'icon', 'requiredPermission']

const isEditing = computed(() => props.editing !== null)

/** The entries an entry may sit under: everything but itself. */
const parents = computed(() =>
  props.entries
    .filter((entry) => entry.id !== props.editing?.id)
    .map((entry) => ({ value: entry.id, label: entry.label })),
)

function messageFor(path: string): string {
  return fieldErrors.value[path] ?? ''
}

watch(
  () => props.open,
  (open) => {
    if (!open) {
      return
    }

    reset()

    const editing = props.editing

    menuKey.value = editing?.menuKey ?? ''
    label.value = editing?.label ?? ''
    icon.value = editing?.icon ?? ''
    routePath.value = editing?.routePath ?? ''
    requiredPermission.value = editing?.requiredPermission ?? ''
    parentId.value = editing?.parentMenuId ?? ''
    sortOrder.value = String(editing?.sortOrder ?? 0)
    isEnabled.value = editing?.isEnabled ?? true
  },
  { immediate: true },
)

function orNull(value: string): string | null {
  const trimmed = value.trim()

  return trimmed === '' ? null : trimmed
}

async function save(): Promise<void> {
  isSaving.value = true
  reset()

  const body = {
    label: label.value.trim(),
    icon: orNull(icon.value),
    parentMenuId: orNull(parentId.value),
    routePath: orNull(routePath.value),
    requiredPermission: orNull(requiredPermission.value),
    sortOrder: Number(sortOrder.value) || 0,
    isEnabled: isEnabled.value,
  }

  try {
    if (props.editing) {
      await updateMenu(props.editing.id, body)
    } else {
      await createMenu({ ...body, menuKey: menuKey.value.trim() })
    }

    emit('saved')
  } catch (failure) {
    report(toApiError(failure))
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    aria-modal="true"
    :aria-label="isEditing ? 'Edit navigation entry' : 'New navigation entry'"
    data-testid="menu-dialog"
  >
    <div class="max-h-full w-full max-w-2xl overflow-y-auto rounded-lg bg-background p-6 shadow-lg">
      <h3 class="text-lg font-semibold">
        {{ isEditing ? 'Edit navigation entry' : 'New navigation entry' }}
      </h3>

      <Alert v-if="formError" variant="destructive" class="mt-4" data-testid="menu-error">
        {{ formError }}
      </Alert>

      <FormUnplacedErrors
        :field-errors="fieldErrors"
        :placed="PLACED"
        class="mt-4"
        data-testid="menu-unplaced-errors"
      />

      <form class="mt-4 grid gap-4 sm:grid-cols-2" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="menu-key">Key</Label>
          <Input id="menu-key" v-model="menuKey" data-testid="menu-key" :disabled="isEditing" />
          <p v-if="isEditing" class="text-xs text-muted-foreground">
            A key cannot change once it names an entry.
          </p>
          <p v-if="messageFor('menuKey')" class="text-xs text-destructive">
            {{ messageFor('menuKey') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="menu-label">Label</Label>
          <Input id="menu-label" v-model="label" data-testid="menu-label" />
          <p v-if="messageFor('label')" class="text-xs text-destructive">
            {{ messageFor('label') }}
          </p>
        </div>

        <div class="space-y-2 sm:col-span-2">
          <Label for="menu-route">Route</Label>
          <Input
            id="menu-route"
            v-model="routePath"
            data-testid="menu-route"
            placeholder="/lists/purchase_requisitions"
          />
          <!-- An entry with no route is a heading, which is what a parent is —
               so this is optional rather than required. -->
          <p class="text-xs text-muted-foreground">
            Somewhere in this application, starting with <code>/</code>. Leave it empty to make this
            a heading for the entries under it.
          </p>
          <p v-if="messageFor('routePath')" class="text-xs text-destructive">
            {{ messageFor('routePath') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="menu-parent">Sits under</Label>
          <Select
            id="menu-parent"
            v-model="parentId"
            data-testid="menu-parent"
            placeholder="Top level"
            :options="parents"
          />
          <p
            v-if="messageFor('parentMenuId')"
            class="text-xs text-destructive"
            data-testid="parent-error"
          >
            {{ messageFor('parentMenuId') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="menu-order">Order</Label>
          <Input id="menu-order" v-model="sortOrder" type="number" data-testid="menu-order" />
        </div>

        <div class="space-y-2">
          <Label for="menu-icon">Icon</Label>
          <Input
            id="menu-icon"
            v-model="icon"
            data-testid="menu-icon"
            placeholder="shopping-cart"
            :invalid="Boolean(messageFor('icon'))"
            :described-by="messageFor('icon') ? 'menu-icon-message' : undefined"
          />
          <p class="text-xs text-muted-foreground">A Lucide icon name.</p>
          <p
            v-if="messageFor('icon')"
            id="menu-icon-message"
            class="text-xs text-destructive"
            data-testid="menu-icon-message"
          >
            {{ messageFor('icon') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="menu-permission">Hidden unless the viewer holds</Label>
          <Input
            id="menu-permission"
            v-model="requiredPermission"
            data-testid="menu-permission"
            placeholder="document:read"
            :invalid="Boolean(messageFor('requiredPermission'))"
            :described-by="messageFor('requiredPermission') ? 'menu-permission-message' : undefined"
          />
          <!-- Stated plainly, because it is the thing somebody will otherwise
               assume: hiding a link does not close the screen behind it. -->
          <p class="text-xs text-muted-foreground">
            This hides the link. The screen it points at enforces its own permission.
          </p>
          <p
            v-if="messageFor('requiredPermission')"
            id="menu-permission-message"
            class="text-xs text-destructive"
            data-testid="menu-permission-message"
          >
            {{ messageFor('requiredPermission') }}
          </p>
        </div>

        <div class="flex items-center gap-2 sm:col-span-2">
          <input
            id="menu-enabled"
            v-model="isEnabled"
            type="checkbox"
            data-testid="menu-enabled"
            class="h-4 w-4"
          />
          <Label for="menu-enabled">Shown in the navigation</Label>
        </div>

        <div class="flex justify-end gap-2 sm:col-span-2">
          <Button type="button" variant="secondary" @click="emit('close')">Cancel</Button>
          <Button type="submit" :disabled="isSaving" data-testid="save-menu">
            {{ isSaving ? 'Saving…' : 'Save' }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
