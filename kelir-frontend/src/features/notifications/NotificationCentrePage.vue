<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import {
  listNotifications,
  markAllNotificationsRead,
  markNotificationRead,
} from '@/api/notifications'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useNotificationStore } from '@/stores/notifications'
import { NOTIFICATION_TYPE_LABELS, type AppNotification } from '@/types/notification'

/**
 * The in-app notification centre (FR-NTF-003; [#251]).
 *
 * **The screen the events of #247 and #248 were not.** Those are a document's
 * timeline, read by somebody who has already opened the document. This is the
 * other direction: what has come to *you*, when you have not opened anything.
 *
 * # What this list is, and what it is not
 *
 * **A record of what reached you, not a live to-do list.** A role task
 * notifies every holder of the role (**D-48**), and the moment one of them
 * claims it the others describe something no longer waiting for them. Those
 * rows stay: they were true when written, and deleting them would take away
 * the answer to *what was I told* while somebody is reading it. **My Tasks is
 * the live view**, and every task notification links to it.
 *
 * # Reading is a side effect of opening, and also a button
 *
 * Following a notification marks it read, because having followed it *is*
 * having read it and a badge that survived the click would be a badge nobody
 * trusts. The explicit control exists for the other case: a person who can see
 * from the list that they do not need to open it.
 *
 * [#251]: https://github.com/sujanto-gaws/kelir/issues/251
 */

const router = useRouter()
const store = useNotificationStore()

const PAGE_SIZE = 20

const items = ref<AppNotification[]>([])
const total = ref(0)
const page = ref(1)
const loading = ref(false)
const failure = ref('')

const more = computed(() => items.value.length < total.value)
const anyUnread = computed(() => items.value.some((item) => item.readAt === null))

/**
 * One page, appended.
 *
 * Newest first, so appending walks backwards through what has happened to you
 * — the direction somebody catching up travels. Replacing would take away the
 * recent entries they came for.
 */
async function load(next: number): Promise<void> {
  loading.value = true
  failure.value = ''

  try {
    const result = await listNotifications(next, PAGE_SIZE)

    items.value = next === 1 ? result.items : [...items.value, ...result.items]
    total.value = result.meta.total
    page.value = next
  } catch (error) {
    failure.value =
      error instanceof ApiError ? error.message : 'Your notifications could not be loaded.'
  } finally {
    loading.value = false
  }
}

function loadMore(): void {
  void load(page.value + 1)
}

/**
 * Marks one read, **locally first**.
 *
 * The row is stamped before the request settles because the person has just
 * clicked it and a list that waits for a round trip to acknowledge a click
 * feels broken. **A failure puts it back** rather than leaving a lie on the
 * screen — and the request is idempotent, so a retry after a network blip
 * costs nothing.
 */
async function markRead(item: AppNotification): Promise<void> {
  if (item.readAt !== null) {
    return
  }

  const previous = item.readAt
  item.readAt = new Date().toISOString()
  store.decrement()

  try {
    await markNotificationRead(item.id)
  } catch (error) {
    item.readAt = previous
    void store.refresh()
    failure.value =
      error instanceof ApiError ? error.message : 'That notification could not be marked read.'
  }
}

async function markAllRead(): Promise<void> {
  try {
    const left = await markAllNotificationsRead()
    const now = new Date().toISOString()

    for (const item of items.value) {
      item.readAt ??= now
    }

    store.set(left.unread)
  } catch (error) {
    void store.refresh()
    failure.value =
      error instanceof ApiError ? error.message : 'Your notifications could not be cleared.'
  }
}

/**
 * Follows a notification to the thing it is about.
 *
 * **The task first, then the document.** A task notification is the one a
 * person can act on, and the inbox is where acting happens; a decision
 * notification has no task of its own to open and goes to the document. One
 * with neither is a message rather than a link, and clicking it only marks it
 * read.
 */
async function open(item: AppNotification): Promise<void> {
  await markRead(item)

  if (item.taskId) {
    await router.push({ name: 'task', params: { id: item.taskId } })
  } else if (item.documentId) {
    await router.push({ name: 'document', params: { id: item.documentId } })
  }
}

function label(item: AppNotification): string {
  return NOTIFICATION_TYPE_LABELS[item.notificationType] ?? 'Notification'
}

function when(item: AppNotification): string {
  return new Date(item.createdAt).toLocaleString()
}

watch(
  () => true,
  () => void load(1),
  { immediate: true },
)
</script>

<template>
  <div class="space-y-4" data-testid="notification-centre">
    <div class="flex items-center gap-3">
      <h2 class="text-lg font-medium">Notifications</h2>

      <Button
        v-if="anyUnread"
        class="ml-auto"
        variant="outline"
        size="sm"
        data-testid="notifications-mark-all"
        @click="markAllRead"
      >
        Mark all as read
      </Button>
    </div>

    <Alert v-if="failure" variant="destructive" data-testid="notifications-error">
      {{ failure }}
    </Alert>

    <p v-if="loading && items.length === 0" class="text-sm text-muted-foreground">
      Loading your notifications…
    </p>

    <p
      v-else-if="items.length === 0 && !failure"
      class="text-sm text-muted-foreground"
      data-testid="notifications-empty"
    >
      Nothing has come to you yet.
    </p>

    <ol v-else class="space-y-2" data-testid="notification-list">
      <li
        v-for="item in items"
        :key="item.id"
        class="rounded-md border p-3"
        :class="item.readAt === null ? 'bg-muted/40' : ''"
        :data-unread="item.readAt === null ? 'true' : 'false'"
        data-testid="notification-row"
      >
        <div class="flex items-start gap-3">
          <button
            type="button"
            class="min-w-0 flex-1 text-left"
            data-testid="notification-open"
            @click="open(item)"
          >
            <p class="text-sm font-medium" data-testid="notification-title">
              {{ item.title }}
            </p>
            <p class="mt-1 text-sm text-muted-foreground" data-testid="notification-body">
              {{ item.body }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              <span
                class="rounded bg-muted px-1.5 py-0.5 font-medium"
                data-testid="notification-type"
              >
                {{ label(item) }}
              </span>
              <span class="ml-2">{{ when(item) }}</span>
            </p>
          </button>

          <Button
            v-if="item.readAt === null"
            variant="ghost"
            size="sm"
            data-testid="notification-mark-read"
            @click="markRead(item)"
          >
            Mark read
          </Button>
        </div>
      </li>
    </ol>

    <Button
      v-if="more"
      variant="outline"
      size="sm"
      :disabled="loading"
      data-testid="notifications-more"
      @click="loadMore"
    >
      Show earlier notifications
    </Button>

    <!-- **The distinction this screen would otherwise blur.** A list of things
         that reached you looks like a list of things to do, and one of them is
         stale the moment somebody else claims the task. My Tasks is the live
         answer; this is the record. -->
    <p class="border-t pt-3 text-xs text-muted-foreground" data-testid="notifications-not-inbox">
      This is what has been sent to you. Whether a task is still open — and still yours to take — is
      answered by My Tasks.
    </p>
  </div>
</template>
