<script setup lang="ts">
import { computed, ref } from 'vue'

import { useFieldScope } from '@/features/rad/renderer/useFieldScope'
import { cn } from '@/lib/utils'
import type { JfssLayoutComponent, JfssTabSlot } from '@/types/jfss'

const props = defineProps<{ component: JfssLayoutComponent }>()

/**
 * Named slots (JFSS §4.3, §4.3.1).
 *
 * **Every tab's components stay mounted.** Only the inactive panels are hidden,
 * rather than rendered on demand — a required field on the third tab must count
 * against the form whether or not anybody opened that tab, and #163's
 * validation and #164's payload both read the whole tree. Rendering the active
 * tab alone would make a form's validity depend on which tabs the user
 * happened to click, which is a defect that only appears on the definitions
 * that need it least.
 *
 * `hidden` rather than `v-show`: the attribute takes the panel out of the
 * accessibility tree as well as out of the layout, so a screen reader does not
 * walk fields the sighted user cannot see.
 */
const tabs = computed<JfssTabSlot[]>(() => props.component.tabs ?? [])

const active = ref(0)

// Scoped for the same reason a field is: a `tabs` container inside a
// repeater's row template renders once per row, and two panels sharing an id
// would leave every tab's `aria-controls` pointing at the first row's.
const scope = useFieldScope()

const panelId = (index: number) => `jfss-${scope.value}${props.component.id}-panel-${index}`
const tabId = (index: number) => `jfss-${scope.value}${props.component.id}-tab-${index}`
</script>

<template>
  <div class="space-y-4">
    <div class="flex flex-wrap gap-1 border-b border-border" role="tablist">
      <button
        v-for="(tab, index) in tabs"
        :id="tabId(index)"
        :key="index"
        type="button"
        role="tab"
        :aria-selected="index === active"
        :aria-controls="panelId(index)"
        :class="
          cn(
            '-mb-px border-b-2 px-3 py-2 text-sm font-medium transition-colors',
            index === active
              ? 'border-primary text-foreground'
              : 'border-transparent text-muted-foreground hover:text-foreground',
          )
        "
        @click="active = index"
      >
        {{ tab.title }}
      </button>
    </div>

    <div
      v-for="(tab, index) in tabs"
      :id="panelId(index)"
      :key="index"
      class="space-y-4"
      role="tabpanel"
      :aria-labelledby="tabId(index)"
      :hidden="index !== active"
    >
      <slot name="group" :components="tab.components ?? []" />
    </div>
  </div>
</template>
