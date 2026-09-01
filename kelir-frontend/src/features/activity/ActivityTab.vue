<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { listActivity } from '@/api/activity'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { EVENT_CATEGORY_LABELS, type ActivityEvent } from '@/types/activity'

const props = defineProps<{ documentId: string }>()

/**
 * The Activity tab of the document workspace (FR-ACT-005; [#250]).
 *
 * **This is MVP criterion 12, and the screen that makes Sprint 12's events
 * worth having.** [#247] and [#248] wrote them and nothing read them — which
 * was the right order, because a timeline with four sources and no screen is
 * still worth having and a screen over one source is not.
 *
 * # It reads through the document's own permission, and nothing else
 *
 * #250 AC2, taken as **D-47**. There is no `can('activity:read')` here and that
 * is the decision rather than an omission: whether this person may see what
 * happened to a document is the same question as whether they may see the
 * document, and the server asks it once. The CommentsTab one panel over *does*
 * gate on `comment:read`, because a comment's text is the comment module's to
 * guard; a document's own history is nobody else's.
 *
 * **Whoever raised the document is the commonest reader of its timeline**, and
 * a deployment has no reason to have granted them a permission named
 * `activity`. Gating this panel would have shown them a refusal on their own
 * document — [#263]'s shape, which this project has now met three times.
 *
 * # Four sources, one list, and the reason that matters (AC3)
 *
 * Document lifecycle, workflow, attachments and comments all land in
 * `activity_events` and all come back from one endpoint, so this renders one
 * list and labels each entry with where it came from. **A timeline showing
 * three of four would be worse than one showing none**: a reader cannot tell an
 * empty category from a missing one, and would take *no attachments were ever
 * added* from a panel that simply had not been taught to ask.
 *
 * # What an entry does not say
 *
 * `details` carries nothing about an entry's *subject* — no file name, no
 * comment length, no second party to a delegation (**D-45**, [#292]). Those are
 * behind their own permissions, and this screen links by id rather than
 * repeating them. So a row reads *a file was attached, by Sara, at 09:05*, and
 * the name is on the Attachments tab, where `attachment:read` decides.
 *
 * That is why this panel renders `actionSummary` — the server's own sentence —
 * rather than composing one from `details`. There is nothing in `details` to
 * compose from, deliberately.
 *
 * [#247]: https://github.com/sujanto-gaws/kelir/issues/247
 * [#248]: https://github.com/sujanto-gaws/kelir/issues/248
 * [#250]: https://github.com/sujanto-gaws/kelir/issues/250
 * [#263]: https://github.com/sujanto-gaws/kelir/issues/263
 * [#292]: https://github.com/sujanto-gaws/kelir/issues/292
 */

/** One page at a time (AC1): a document that ran for a year fills any list. */
const PAGE_SIZE = 20

const events = ref<ActivityEvent[]>([])
const total = ref(0)
const page = ref(1)
const loading = ref(false)
const failure = ref('')

const more = computed(() => events.value.length < total.value)

/**
 * One page, **appended** rather than replaced.
 *
 * The list is newest first, so appending walks backwards through the
 * document's life — which is the direction somebody reading *what has happened
 * here* travels. Replacing the page would take away the recent entries they
 * came for in order to show them older ones.
 *
 * **A page boundary cannot show a row twice or skip one** (AC6), and that is
 * the server's doing rather than this function's: the read orders by
 * `created_at DESC, id DESC`, a total order, because two events written in one
 * transaction share a timestamp and `created_at` alone would leave their
 * relative order to the planner. That is `workflow_history`'s lesson from
 * [#181](https://github.com/sujanto-gaws/kelir/issues/181).
 */
async function load(id: string, next: number): Promise<void> {
  loading.value = true
  failure.value = ''

  try {
    const result = await listActivity(id, next, PAGE_SIZE)

    events.value = next === 1 ? result.items : [...events.value, ...result.items]
    total.value = result.meta.total
    page.value = next
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'The activity could not be loaded.'
  } finally {
    loading.value = false
  }
}

function loadMore(): void {
  void load(props.documentId, page.value + 1)
}

/**
 * The actor, as the row recorded them (AC4).
 *
 * **Not joined to `users` now.** `actorName` is what this person was called
 * when the thing happened, so a rename does not rewrite the past and a removed
 * account does not blank a history it took part in. The fallback is for the
 * rows the system itself wrote, which have no actor at all.
 */
function actor(event: ActivityEvent): string {
  return event.actorName ?? 'The system'
}

function when(event: ActivityEvent): string {
  return new Date(event.occurredAt).toLocaleString()
}

function category(event: ActivityEvent): string {
  return EVENT_CATEGORY_LABELS[event.eventCategory] ?? event.eventCategory
}

watch(
  () => props.documentId,
  (id) => {
    events.value = []
    total.value = 0
    page.value = 1
    void load(id, 1)
  },
  { immediate: true },
)
</script>

<template>
  <div class="space-y-4" data-testid="activity-tab">
    <Alert v-if="failure" variant="destructive" data-testid="activity-error">
      {{ failure }}
    </Alert>

    <p v-if="loading && events.length === 0" class="text-sm text-muted-foreground">
      Loading the activity…
    </p>

    <p
      v-else-if="events.length === 0 && !failure"
      class="text-sm text-muted-foreground"
      data-testid="activity-empty"
    >
      Nothing has been recorded against this document yet.
    </p>

    <!-- **Newest first**, which is the order the API returns and the order a
         person opening this tab is asking about: *what has happened lately*.
         The Comments tab is oldest-first for the opposite reason, and the two
         differ deliberately. -->
    <ol v-else class="space-y-3" data-testid="activity-list">
      <li
        v-for="event in events"
        :key="event.id"
        class="rounded-md border p-3"
        data-testid="activity-row"
      >
        <p class="text-sm">
          <!-- The server's own sentence. Composing one here from `details`
               would be composing from an object D-45 emptied on purpose. -->
          <span class="font-medium" data-testid="activity-summary">
            {{ event.actionSummary }}
          </span>
          <span class="text-muted-foreground"> · {{ actor(event) }} · {{ when(event) }} </span>
        </p>

        <p class="mt-1 text-xs text-muted-foreground">
          <!-- AC3 made visible. The label is what lets a reader see that all
               four sources are here — without it, a timeline of only document
               events and a timeline missing three sources look identical. -->
          <span class="rounded bg-muted px-1.5 py-0.5 font-medium" data-testid="activity-category">
            {{ category(event) }}
          </span>
          <span class="ml-2" data-testid="activity-event-type">{{ event.eventType }}</span>
        </p>
      </li>
    </ol>

    <Button
      v-if="more"
      variant="outline"
      size="sm"
      :disabled="loading"
      data-testid="activity-more"
      @click="loadMore"
    >
      Show earlier activity
    </Button>

    <!-- **AC5, and this is the surface where somebody would otherwise merge
         them.** #247 states the distinction in four places nobody reading this
         screen will open. A person looking at a list of who did what to a
         document, on a screen next to a tab called History, is exactly the
         person about to assume this is the compliance record — and act on that
         assumption in a conversation with an auditor. -->
    <p class="border-t pt-3 text-xs text-muted-foreground" data-testid="activity-not-audit">
      This is the document's activity, not the audit trail. It shows what happened here, for the
      people working on this document. The audit trail is a separate, hash-chained record of whether
      data was tampered with, behind its own permission — and neither is derived from the other. The
      History tab is different again: it shows only this document's status changes.
    </p>
  </div>
</template>
