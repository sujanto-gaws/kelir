<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { addComment, listComments } from '@/api/comments'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { MAX_COMMENT_BODY, type Comment } from '@/types/comment'

const props = defineProps<{ documentId: string }>()
const auth = useAuthStore()

/**
 * The Comments tab of the document workspace (FR-CMT-001; [#296]).
 *
 * **This is the screen SRS §9 criterion 11 has been claiming since
 * 2026-08-11.** The API shipped in Sprint 12; *comments can be added* was true
 * of an endpoint and of nobody. It replaces the last placeholder
 * [#172](https://github.com/sujanto-gaws/kelir/issues/172) left, which is why
 * that placeholder existed rather than a blank panel — and with it the workspace
 * stops promising anything a later phase will fill.
 *
 * # The distinction this screen will be asked to blur
 *
 * `modules::comment` states it, `0032_comment.sql` states it, and a row-level
 * test asserts it: **this is not the decision comment.** FR-TASK-006 shipped in
 * Sprint 11 as three columns written with an approval and immutable because the
 * decision is; this is a conversation.
 *
 * **The workspace makes merging them the obvious mistake**, because the
 * Workflow tab one panel over renders decisions *with their reasons* and they
 * look like comments with authors. They are not, and the two live in different
 * tabs deliberately. The note under the composer says so to the person, because
 * this project has drawn that line four times in prose and this is the first
 * time somebody can see it.
 *
 * # What is not here yet
 *
 * **No threading, editing, deleting or resolving** — FR-CMT-002/003/004 are
 * [#253](https://github.com/sujanto-gaws/kelir/issues/253), and `parent_comment_id`
 * and `status` exist unwritten for them. A reply control that posted a
 * top-level comment would be worse than no reply control.
 *
 * [#296]: https://github.com/sujanto-gaws/kelir/issues/296
 */

const comments = ref<Comment[]>([])
const draft = ref('')
const loading = ref(false)
const posting = ref(false)
const failure = ref<string | null>(null)
const postFailure = ref<string | null>(null)

const canRead = computed(() => auth.can('comment:read'))
const canWrite = computed(() => auth.can('comment:create'))
const tooLong = computed(() => draft.value.trim().length > MAX_COMMENT_BODY)
const empty = computed(() => draft.value.trim().length === 0)

async function load(): Promise<void> {
  if (!canRead.value) {
    return
  }

  loading.value = true
  failure.value = null

  try {
    const page = await listComments(props.documentId)
    comments.value = page.items
  } catch (error) {
    failure.value =
      error instanceof ApiError ? error.message : 'The conversation could not be loaded.'
  } finally {
    loading.value = false
  }
}

/**
 * Adds the drafted comment.
 *
 * **The draft survives a refusal.** A comment somebody has written is work, and
 * clearing the box on a 422 would throw it away at the moment they most want it
 * back. It is cleared only once the server has the row.
 */
async function post(): Promise<void> {
  if (empty.value || tooLong.value) {
    return
  }

  posting.value = true
  postFailure.value = null

  try {
    await addComment(props.documentId, draft.value)
    draft.value = ''
    await load()
  } catch (error) {
    postFailure.value =
      error instanceof ApiError ? error.message : 'The comment could not be added.'
  } finally {
    posting.value = false
  }
}

function said(comment: Comment): string {
  return new Date(comment.createdAt).toLocaleString()
}

watch(() => props.documentId, load, { immediate: true })
</script>

<template>
  <div class="space-y-4" data-testid="comments-tab">
    <p v-if="!canRead" class="text-sm text-muted-foreground" data-testid="comments-forbidden">
      You do not have permission to see this document's comments.
    </p>

    <template v-else>
      <Alert v-if="failure" variant="destructive" data-testid="comments-error">
        {{ failure }}
      </Alert>

      <p v-if="loading" class="text-sm text-muted-foreground">Loading the conversation…</p>

      <p
        v-else-if="comments.length === 0"
        class="text-sm text-muted-foreground"
        data-testid="comments-empty"
      >
        Nobody has commented on this document yet.
      </p>

      <!-- **Oldest first**, which is the order the API returns and the order a
           conversation is read in. Every other list in this product is newest
           first, and that difference is deliberate. -->
      <ol v-else class="space-y-3" data-testid="comment-list">
        <li
          v-for="comment in comments"
          :key="comment.id"
          class="rounded-md border p-3"
          data-testid="comment-row"
        >
          <p class="text-sm font-medium">
            {{ comment.authorUsername ?? 'Somebody who has since been removed' }}
            <span class="font-normal text-muted-foreground"> · {{ said(comment) }}</span>
          </p>
          <!-- Interpolated, never rendered as markup: a comment is text one
               person wrote for others to read, and the server escaping it is
               not a licence to interpolate it into anything. -->
          <p class="mt-1 whitespace-pre-wrap text-sm" data-testid="comment-body">
            {{ comment.body }}
          </p>
        </li>
      </ol>

      <div v-if="canWrite" class="space-y-2">
        <label class="text-sm font-medium" for="comment-body">Add a comment</label>
        <textarea
          id="comment-body"
          v-model="draft"
          rows="3"
          class="w-full rounded-md border p-2 text-sm"
          data-testid="comment-input"
          :disabled="posting"
        />

        <!-- **The line this screen exists to keep visible.** The Workflow tab
             one panel over shows decisions with their reasons, which look like
             comments with authors and are not: a reason is part of a decision
             and cannot be edited, where this is a conversation. -->
        <p class="text-xs text-muted-foreground" data-testid="comment-distinction">
          A comment is part of the conversation about this document. The reason an approver gives
          with a decision is recorded with that decision, on the Workflow tab.
        </p>

        <Alert v-if="postFailure" variant="destructive" data-testid="comment-error">
          {{ postFailure }}
        </Alert>

        <p v-if="tooLong" class="text-xs text-destructive" data-testid="comment-too-long">
          A comment is at most {{ MAX_COMMENT_BODY }} characters.
        </p>

        <Button
          size="sm"
          :disabled="posting || empty || tooLong"
          data-testid="comment-submit"
          @click="post"
        >
          Add comment
        </Button>
      </div>
    </template>
  </div>
</template>
