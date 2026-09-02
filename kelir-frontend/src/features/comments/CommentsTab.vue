<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { addComment, deleteComment, editComment, listComments } from '@/api/comments'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { MAX_COMMENT_BODY, type Comment } from '@/types/comment'

const props = defineProps<{ documentId: string }>()
const auth = useAuthStore()

/**
 * The Comments tab of the document workspace (FR-CMT-001 to FR-CMT-004; [#296],
 * [#253]).
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
 * **The tail made that line visible rather than blurring it** (#253): a comment
 * here can now be replied to, edited and deleted, and every one of those is
 * something a decision's reason may never be. The distinction note stays under
 * the composer for exactly that reason.
 *
 * # Three shapes this screen renders that the first one did not
 *
 * **A reply reads under what it answers** (**D-50**, one level). The server
 * returns the conversation thread-major — a root, then its replies, then the
 * next root — so this groups rather than sorts, and a reply whose root is on
 * another page is drawn at the top level rather than dropped.
 *
 * **A tombstone.** A deleted comment that still has replies comes back with
 * `body: null` (**D-51**), and is drawn as *this comment was deleted* with its
 * author and time and no actions. Rendering it as an empty bubble would be a
 * comment of nothing, which the API refuses to store; dropping it would orphan
 * the answers under it.
 *
 * **An edit says it is one.** `editedAt` is stamped by the server when the body
 * changes and by nothing else, and it is shown next to the time. A comment whose
 * text changed with nothing saying so is a conversation somebody can rewrite
 * after the fact.
 *
 * # Deleting asks first, and asks in the page
 *
 * The confirmation is a second button in the row, not `window.confirm`: a
 * blocking browser dialog is untestable here, unstyled, and cannot say what is
 * about to happen — *the replies stay* is the part somebody deleting a root
 * needs to know, and a modal that only says "OK?" cannot tell them.
 *
 * [#253]: https://github.com/sujanto-gaws/kelir/issues/253
 * [#296]: https://github.com/sujanto-gaws/kelir/issues/296
 */

const comments = ref<Comment[]>([])
const draft = ref('')
const loading = ref(false)
const posting = ref(false)
const failure = ref<string | null>(null)
const postFailure = ref<string | null>(null)

/** The root being replied to, and the reply being written for it. */
const replyingTo = ref<string | null>(null)
const replyDraft = ref('')
const replyFailure = ref<string | null>(null)

/** The comment being edited, and the text it will become. */
const editing = ref<string | null>(null)
const editDraft = ref('')
const editFailure = ref<string | null>(null)

/** The comment whose delete has been asked for and not yet confirmed. */
const confirming = ref<string | null>(null)
const deleteFailure = ref<string | null>(null)

/** True while any of the three writes is in flight, so nothing double-fires. */
const working = ref(false)

const canRead = computed(() => auth.can('comment:read'))
const canWrite = computed(() => auth.can('comment:create'))
const canEdit = computed(() => auth.can('comment:update'))
const canDelete = computed(() => auth.can('comment:delete'))
const tooLong = computed(() => draft.value.trim().length > MAX_COMMENT_BODY)
const empty = computed(() => draft.value.trim().length === 0)
const replyTooLong = computed(() => replyDraft.value.trim().length > MAX_COMMENT_BODY)
const replyEmpty = computed(() => replyDraft.value.trim().length === 0)
const editTooLong = computed(() => editDraft.value.trim().length > MAX_COMMENT_BODY)
const editEmpty = computed(() => editDraft.value.trim().length === 0)

/**
 * The conversation as a list of threads.
 *
 * **Grouped, not sorted.** The order is the server's — a root, then what answers
 * it — and re-sorting here would be a second opinion about a thing the API has
 * already decided. A reply whose root is not on this page has nowhere to nest,
 * so it stands as its own row: an answer shown out of place is still an answer,
 * and dropping it would hide somebody's words behind a page boundary.
 */
const threads = computed(() => {
  const roots: { comment: Comment; replies: Comment[] }[] = []
  const byId = new Map<string, { comment: Comment; replies: Comment[] }>()

  for (const comment of comments.value) {
    const parent = comment.parentCommentId ? byId.get(comment.parentCommentId) : undefined

    if (parent) {
      parent.replies.push(comment)
      continue
    }

    const thread = { comment, replies: [] as Comment[] }
    roots.push(thread)
    byId.set(comment.id, thread)
  }

  return roots
})

/** Whether this comment is one the signed-in person wrote. */
function mine(comment: Comment): boolean {
  return comment.authorUserId !== null && comment.authorUserId === auth.user?.id
}

/**
 * A tombstone: deleted, kept because replies hang from it, and body withheld.
 *
 * **Truthiness rather than `!== null`**, which is not laziness: an older client
 * bundle reading a payload without the field would otherwise read every comment
 * as deleted, and a screen that hides the whole conversation on a missing key is
 * a worse failure than one that shows it.
 */
function deleted(comment: Comment): boolean {
  return Boolean(comment.deletedAt)
}

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

function startReply(comment: Comment): void {
  replyingTo.value = comment.id
  replyDraft.value = ''
  replyFailure.value = null
  editing.value = null
  confirming.value = null
}

function cancelReply(): void {
  replyingTo.value = null
  replyDraft.value = ''
  replyFailure.value = null
}

/** Sends the reply, and keeps the draft if the server refuses it. */
async function sendReply(parentId: string): Promise<void> {
  if (replyEmpty.value || replyTooLong.value) {
    return
  }

  working.value = true
  replyFailure.value = null

  try {
    await addComment(props.documentId, replyDraft.value, parentId)
    cancelReply()
    await load()
  } catch (error) {
    replyFailure.value = error instanceof ApiError ? error.message : 'The reply could not be added.'
  } finally {
    working.value = false
  }
}

function startEdit(comment: Comment): void {
  editing.value = comment.id
  editDraft.value = comment.body ?? ''
  editFailure.value = null
  replyingTo.value = null
  confirming.value = null
}

function cancelEdit(): void {
  editing.value = null
  editDraft.value = ''
  editFailure.value = null
}

async function saveEdit(commentId: string): Promise<void> {
  if (editEmpty.value || editTooLong.value) {
    return
  }

  working.value = true
  editFailure.value = null

  try {
    await editComment(props.documentId, commentId, editDraft.value)
    cancelEdit()
    await load()
  } catch (error) {
    editFailure.value =
      error instanceof ApiError ? error.message : 'The comment could not be edited.'
  } finally {
    working.value = false
  }
}

/**
 * Deletes, after the second click.
 *
 * **The list is reloaded rather than spliced**, because what a delete leaves
 * behind is the server's answer: a comment with replies stays as a tombstone and
 * one without disappears, and a screen guessing between those two would show the
 * wrong conversation until the next load.
 */
async function confirmDelete(commentId: string): Promise<void> {
  working.value = true
  deleteFailure.value = null

  try {
    await deleteComment(props.documentId, commentId)
    confirming.value = null
    await load()
  } catch (error) {
    deleteFailure.value =
      error instanceof ApiError ? error.message : 'The comment could not be deleted.'
  } finally {
    working.value = false
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
           first, and that difference is deliberate. Replies are nested under
           what they answer, one level and no deeper. -->
      <ol v-else class="space-y-3" data-testid="comment-list">
        <li v-for="thread in threads" :key="thread.comment.id">
          <div class="rounded-md border p-3" data-testid="comment-row">
            <p class="text-sm font-medium">
              {{ thread.comment.authorUsername ?? 'Somebody who has since been removed' }}
              <span class="font-normal text-muted-foreground">
                · {{ said(thread.comment) }}
                <!-- An edit is visible as an edit: a comment whose text changed
                     with nothing saying so is a conversation somebody can
                     rewrite after the fact. -->
                <span v-if="thread.comment.editedAt" data-testid="comment-edited"> · edited</span>
              </span>
            </p>

            <!-- A deleted comment with replies keeps its place so the answers
                 under it still have something to answer. -->
            <p
              v-if="deleted(thread.comment)"
              class="mt-1 text-sm italic text-muted-foreground"
              data-testid="comment-deleted"
            >
              This comment was deleted. The replies to it are still here.
            </p>

            <template v-else-if="editing === thread.comment.id">
              <textarea
                v-model="editDraft"
                rows="3"
                class="mt-1 w-full rounded-md border p-2 text-sm"
                data-testid="edit-input"
                :disabled="working"
              />
              <p v-if="editTooLong" class="text-xs text-destructive" data-testid="edit-too-long">
                A comment is at most {{ MAX_COMMENT_BODY }} characters.
              </p>
              <Alert v-if="editFailure" variant="destructive" data-testid="edit-error">
                {{ editFailure }}
              </Alert>
              <div class="mt-2 flex gap-2">
                <Button
                  size="sm"
                  :disabled="working || editEmpty || editTooLong"
                  data-testid="edit-submit"
                  @click="saveEdit(thread.comment.id)"
                >
                  Save
                </Button>
                <Button size="sm" variant="ghost" data-testid="edit-cancel" @click="cancelEdit">
                  Cancel
                </Button>
              </div>
            </template>

            <!-- Interpolated, never rendered as markup: a comment is text one
                 person wrote for others to read, and the server escaping it is
                 not a licence to interpolate it into anything. -->
            <p v-else class="mt-1 whitespace-pre-wrap text-sm" data-testid="comment-body">
              {{ thread.comment.body }}
            </p>

            <div
              v-if="!deleted(thread.comment) && editing !== thread.comment.id"
              class="mt-2 flex gap-2"
            >
              <Button
                v-if="canWrite"
                size="sm"
                variant="ghost"
                data-testid="comment-reply"
                @click="startReply(thread.comment)"
              >
                Reply
              </Button>
              <Button
                v-if="mine(thread.comment) && canEdit"
                size="sm"
                variant="ghost"
                data-testid="comment-edit"
                @click="startEdit(thread.comment)"
              >
                Edit
              </Button>
              <template v-if="mine(thread.comment) && canDelete">
                <Button
                  v-if="confirming !== thread.comment.id"
                  size="sm"
                  variant="ghost"
                  data-testid="comment-delete"
                  @click="confirming = thread.comment.id"
                >
                  Delete
                </Button>
                <template v-else>
                  <!-- The replies survive, and somebody deleting a comment that
                       has them is told so before they do it. -->
                  <span class="self-center text-xs text-muted-foreground" data-testid="delete-ask">
                    Delete this comment? Any replies to it stay.
                  </span>
                  <Button
                    size="sm"
                    variant="destructive"
                    :disabled="working"
                    data-testid="comment-delete-confirm"
                    @click="confirmDelete(thread.comment.id)"
                  >
                    Delete
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    data-testid="comment-delete-cancel"
                    @click="confirming = null"
                  >
                    Keep
                  </Button>
                </template>
              </template>
            </div>

            <Alert
              v-if="deleteFailure && confirming === thread.comment.id"
              variant="destructive"
              data-testid="delete-error"
            >
              {{ deleteFailure }}
            </Alert>

            <div v-if="replyingTo === thread.comment.id" class="mt-2 space-y-2">
              <label class="text-sm font-medium" :for="`reply-${thread.comment.id}`">
                Reply to {{ thread.comment.authorUsername ?? 'this comment' }}
              </label>
              <textarea
                :id="`reply-${thread.comment.id}`"
                v-model="replyDraft"
                rows="2"
                class="w-full rounded-md border p-2 text-sm"
                data-testid="reply-input"
                :disabled="working"
              />
              <p v-if="replyTooLong" class="text-xs text-destructive" data-testid="reply-too-long">
                A comment is at most {{ MAX_COMMENT_BODY }} characters.
              </p>
              <Alert v-if="replyFailure" variant="destructive" data-testid="reply-error">
                {{ replyFailure }}
              </Alert>
              <div class="flex gap-2">
                <Button
                  size="sm"
                  :disabled="working || replyEmpty || replyTooLong"
                  data-testid="reply-submit"
                  @click="sendReply(thread.comment.id)"
                >
                  Reply
                </Button>
                <Button size="sm" variant="ghost" data-testid="reply-cancel" @click="cancelReply">
                  Cancel
                </Button>
              </div>
            </div>
          </div>

          <!-- One level, and the indent is the whole of the nesting: a reply to
               a reply is refused by the server, so there is no third depth for
               this template to have to draw. -->
          <ol v-if="thread.replies.length > 0" class="mt-2 space-y-2 pl-6" data-testid="reply-list">
            <li
              v-for="answer in thread.replies"
              :key="answer.id"
              class="rounded-md border p-3"
              data-testid="comment-reply-row"
            >
              <p class="text-sm font-medium">
                {{ answer.authorUsername ?? 'Somebody who has since been removed' }}
                <span class="font-normal text-muted-foreground">
                  · {{ said(answer) }}
                  <span v-if="answer.editedAt" data-testid="comment-edited"> · edited</span>
                </span>
              </p>

              <template v-if="editing === answer.id">
                <textarea
                  v-model="editDraft"
                  rows="3"
                  class="mt-1 w-full rounded-md border p-2 text-sm"
                  data-testid="edit-input"
                  :disabled="working"
                />
                <p v-if="editTooLong" class="text-xs text-destructive">
                  A comment is at most {{ MAX_COMMENT_BODY }} characters.
                </p>
                <Alert v-if="editFailure" variant="destructive" data-testid="edit-error">
                  {{ editFailure }}
                </Alert>
                <div class="mt-2 flex gap-2">
                  <Button
                    size="sm"
                    :disabled="working || editEmpty || editTooLong"
                    data-testid="edit-submit"
                    @click="saveEdit(answer.id)"
                  >
                    Save
                  </Button>
                  <Button size="sm" variant="ghost" data-testid="edit-cancel" @click="cancelEdit">
                    Cancel
                  </Button>
                </div>
              </template>

              <p v-else class="mt-1 whitespace-pre-wrap text-sm" data-testid="comment-body">
                {{ answer.body }}
              </p>

              <!-- No Reply control here. The server refuses a reply to a reply
                   (D-50) and a button that posted one would be a control whose
                   only outcome is a 422. -->
              <div v-if="mine(answer) && editing !== answer.id" class="mt-2 flex gap-2">
                <Button
                  v-if="canEdit"
                  size="sm"
                  variant="ghost"
                  data-testid="comment-edit"
                  @click="startEdit(answer)"
                >
                  Edit
                </Button>
                <template v-if="canDelete">
                  <Button
                    v-if="confirming !== answer.id"
                    size="sm"
                    variant="ghost"
                    data-testid="comment-delete"
                    @click="confirming = answer.id"
                  >
                    Delete
                  </Button>
                  <template v-else>
                    <span
                      class="self-center text-xs text-muted-foreground"
                      data-testid="delete-ask"
                    >
                      Delete this reply?
                    </span>
                    <Button
                      size="sm"
                      variant="destructive"
                      :disabled="working"
                      data-testid="comment-delete-confirm"
                      @click="confirmDelete(answer.id)"
                    >
                      Delete
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      data-testid="comment-delete-cancel"
                      @click="confirming = null"
                    >
                      Keep
                    </Button>
                  </template>
                </template>
              </div>
            </li>
          </ol>
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
             and cannot be edited, where this is a conversation — which, since
             #253, is something anyone here can edit and delete. -->
        <p class="text-xs text-muted-foreground" data-testid="comment-distinction">
          A comment is part of the conversation about this document, and its author can edit or
          delete it. The reason an approver gives with a decision is recorded with that decision, on
          the Workflow tab, and cannot be changed afterwards.
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
