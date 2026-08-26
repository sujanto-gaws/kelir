<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'

import FieldShell from './FieldShell.vue'
import { listLookupOptions } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { useLookupSource } from '@/features/rad/renderer/useLookupBindings'
import type { JfssDataComponent } from '@/types/jfss'
import type { LookupOption } from '@/types/rad'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * The one component that reaches outside JFSS (FR-RAD-007, #161).
 *
 * Every other field renders what the definition says. This one asks the server
 * what the choices are, which is why it was built before the renderer treated
 * every component the same way.
 *
 * **The search runs on the server.** `/rad/lookups/{source}/options` pages and
 * filters, and it enforces the permission each underlying master-data endpoint
 * requires — so a lookup cannot become a way to read records the caller could
 * not read directly (**D-12**, and the reasoning #97 and #161 have now applied
 * three times). Fetching a population and filtering it here would defeat the
 * paging and leave the permission check the only thing still working.
 */
const source = useLookupSource(props.component.id)

const options = ref<LookupOption[]>([])
const search = ref('')
const loading = ref(false)
const failed = ref(false)

/** The current value's label, kept so a stored id renders as a name. */
const selectedLabel = computed(
  () => options.value.find((option) => option.value === props.modelValue)?.label,
)

const choices = computed(() =>
  options.value.map((option) => ({
    value: option.value,
    // The business identifier disambiguates two records a person calls the
    // same thing, which is the case a chooser over master data actually hits.
    label: option.description ? `${option.label} — ${option.description}` : option.label,
  })),
)

const selected = computed({
  get: () => (props.modelValue == null ? '' : String(props.modelValue)),
  set: (next: string) => emit('update:modelValue', next === '' ? null : next),
})

async function load(): Promise<void> {
  if (!source) {
    return
  }

  loading.value = true
  failed.value = false

  try {
    const page = await listLookupOptions(source, { search: search.value })
    options.value = page.items
  } catch {
    // The message is not composed here (coding standard §3.3) and the error is
    // not rethrown: one lookup that cannot reach its source must not take down
    // a form whose other twelve fields are fine.
    failed.value = true
    options.value = []
  } finally {
    loading.value = false
  }
}

onMounted(load)

/**
 * Re-query as the search changes, after a pause.
 *
 * 250 ms because the endpoint is a database query per keystroke otherwise, and
 * a chooser is typed into rather than pasted into.
 */
let pending: ReturnType<typeof setTimeout> | undefined

watch(search, () => {
  clearTimeout(pending)
  pending = setTimeout(load, 250)
})
</script>

<template>
  <FieldShell v-slot="{ controlId, describedBy }" :component="component">
    <div class="space-y-2">
      <!-- No binding is a definition the backend should have refused, so it is
           reported rather than rendered as an empty chooser — which would read
           as "master data holds nothing". -->
      <Alert v-if="!source" variant="destructive">
        This lookup field names no master-data source.
      </Alert>

      <template v-else>
        <Input
          v-model="search"
          type="search"
          :placeholder="component.placeholder ?? 'Search…'"
          :disabled="component.readOnly"
          :aria-label="`Search ${component.label}`"
        />

        <Select
          :id="controlId"
          v-model="selected"
          :options="choices"
          :disabled="component.readOnly || loading"
          :described-by="describedBy"
          placeholder="Select…"
        />

        <p v-if="loading" class="text-sm text-muted-foreground">Loading…</p>

        <Alert v-else-if="failed" variant="destructive">
          These choices could not be loaded.
        </Alert>

        <!-- A value stored earlier whose record is not in the current page: the
             chooser would otherwise show "Select…" over a field that holds a
             perfectly good id, which reads as data loss. -->
        <p
          v-else-if="modelValue != null && selectedLabel === undefined"
          class="text-sm text-muted-foreground"
        >
          Currently set to a record outside these results.
        </p>
      </template>
    </div>
  </FieldShell>
</template>
