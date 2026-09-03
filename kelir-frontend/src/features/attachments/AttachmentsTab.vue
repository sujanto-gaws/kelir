<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'

import {
  addReference,
  deleteAttachment,
  deleteReference,
  downloadAttachment,
  listAttachments,
  listCategories,
  listReferences,
  uploadAttachment,
} from '@/api/attachments'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import {
  isDownloadable,
  safeHref,
  SCAN_STATUS_EXPLANATIONS,
  SCAN_STATUS_LABELS,
  type Attachment,
  type AttachmentCategory,
  type ExternalReference,
} from '@/types/attachment'

const props = defineProps<{ documentId: string }>()
const auth = useAuthStore()

/**
 * The Attachments tab of the document workspace (FR-ATT-001..003, FR-ATT-006,
 * FR-ATT-009, FR-ATT-010; [#295], [#254]).
 *
 * **This is the screen SRS §9 criterion 6 has been claiming since 2026-08-11.**
 * The API shipped in Sprint 12; *users can upload attachments* was true of an
 * endpoint and of nobody, and a criterion that says **users can** is not met by
 * one. It replaces the placeholder [#172](https://github.com/sujanto-gaws/kelir/issues/172)
 * left saying *Phase 6 fills this*, which is why that placeholder existed.
 *
 * # The scan states are the feature, not a detail of it
 *
 * An attachment is not downloadable until something has cleared it, and
 * `PENDING`, `INFECTED` and `FAILED` are three refusals rather than three
 * stages of one ([#246](https://github.com/sujanto-gaws/kelir/issues/246) AC3).
 * **A screen that renders one spinner for all three turns a security control
 * into a bug report** — the person waiting is told to wait, and the person
 * holding an infected file is told the same thing for ever.
 *
 * `FAILED` is the one most easily got wrong, because it looks like this
 * product's error rather than the file's. It is not: a scan that could not run
 * has cleared nothing, so the file is refused exactly as an infected one is,
 * and the words say so.
 *
 * # A link is in this list and is visibly not a file ([#254] AC4)
 *
 * The tail added external references, and they are shown **here** rather than in
 * a tab of their own: a person looking for the quotation does not know or care
 * whether it arrived as a PDF or as a link to the vendor's portal. What they
 * must not do is mistake one for the other, so a reference carries a **Link**
 * badge, **no size, no scan status and no download** — it has an *Open* that
 * leaves this product — and the type it is rendered from has no such fields to
 * render (`ExternalReference`). AC5 needs no code here at all: there is no scan
 * status on a reference to read as `CLEAN`.
 *
 * **`safeHref` is defence in depth and says so.** The server refuses anything
 * but http and https; this refuses to put anything else in an `href`, because a
 * `javascript:` link is script in this page with this session.
 *
 * # What the browser does not decide
 *
 * **The download button follows the server's status and nothing else.** There
 * is no client-side rule about which files are safe; `isDownloadable` reads one
 * field, and the API refuses anything else with a 409 whatever this screen
 * renders. That is deliberate: the gate is enforced where the bytes are served
 * ([#246] AC4), and a screen is a convenience over it rather than a second
 * copy of it. The same is true of the delete: the button appears for the
 * uploader, and the server refuses everybody else whatever this renders.
 *
 * **And the two refusals at the door are the server's words.** A size limit and
 * an allowed-type list live in configuration; re-deriving either here would be
 * a second policy that drifts from the one that decides.
 *
 * [#295]: https://github.com/sujanto-gaws/kelir/issues/295
 * [#254]: https://github.com/sujanto-gaws/kelir/issues/254
 * [#246]: https://github.com/sujanto-gaws/kelir/issues/246
 */

const attachments = ref<Attachment[]>([])
const references = ref<ExternalReference[]>([])
const categories = ref<AttachmentCategory[]>([])
const loading = ref(false)
const failure = ref<string | null>(null)
const uploadFailure = ref<string | null>(null)
const uploading = ref(false)
const chosenCategory = ref('')

/** The link being written, and what the server said about the last one. */
const linkLabel = ref('')
const linkUrl = ref('')
const linkCategory = ref('')
const linkFailure = ref<string | null>(null)
const linking = ref(false)
const composingLink = ref(false)

/** The row whose delete has been asked for and not yet confirmed. */
const confirming = ref<string | null>(null)
const deleteFailure = ref<string | null>(null)

const canRead = computed(() => auth.can('attachment:read'))
const canUpload = computed(() => auth.can('attachment:create'))
const canDelete = computed(() => auth.can('attachment:delete'))
const canLink = computed(() => auth.can('attachment:reference'))
const linkIncomplete = computed(
  () => linkLabel.value.trim().length === 0 || linkUrl.value.trim().length === 0,
)

/** Whether this row is one the signed-in person put here. */
function mine(row: { createdBy: string | null }): boolean {
  return row.createdBy !== null && row.createdBy === auth.user?.id
}

async function load(): Promise<void> {
  if (!canRead.value) {
    return
  }

  loading.value = true
  failure.value = null

  // **Two calls, settled independently.** They are two collections on the
  // server because a file and a link are different things (D-53), and this
  // screen shows them together — but *together* must not mean *both or
  // neither*: a references endpoint that fails, or one an older backend does
  // not have during a rolling deploy, would otherwise blank a list of files
  // that loaded perfectly well. Each list takes its own result, and one banner
  // says something did not load.
  const [files, links] = await Promise.allSettled([
    listAttachments(props.documentId),
    listReferences(props.documentId),
  ])

  if (files.status === 'fulfilled') {
    attachments.value = files.value.items
  } else {
    failure.value =
      files.reason instanceof ApiError
        ? files.reason.message
        : 'The attachments could not be loaded.'
  }

  if (links.status === 'fulfilled') {
    references.value = links.value.items
  } else if (!failure.value) {
    failure.value =
      links.reason instanceof ApiError
        ? links.reason.message
        : 'The links on this document could not be loaded.'
  }

  loading.value = false
}

/**
 * The categories, fetched once.
 *
 * A failure here is **not** shown: the picker is a convenience, an upload
 * without a category is a normal outcome, and an error banner about a dropdown
 * would be louder than what it costs.
 */
async function loadCategories(): Promise<void> {
  if (!canRead.value) {
    return
  }

  try {
    categories.value = await listCategories()
  } catch {
    categories.value = []
  }
}

/**
 * Uploads the chosen file.
 *
 * **The server's refusal is shown verbatim**, because it is the only place that
 * knows the limit and the allowed types: a 422 naming the size says what the
 * limit is, and one naming the type says what the file turned out to be rather
 * than what it was called.
 */
async function upload(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]

  if (!file) {
    return
  }

  uploading.value = true
  uploadFailure.value = null

  try {
    await uploadAttachment(props.documentId, file, undefined, chosenCategory.value || undefined)
    await load()
  } catch (error) {
    uploadFailure.value =
      error instanceof ApiError ? error.message : 'The file could not be attached.'
  } finally {
    uploading.value = false
    // So the same file can be chosen again after a refusal was corrected.
    input.value = ''
  }
}

/** Records a link, keeping what was typed if the server refuses it. */
async function link(): Promise<void> {
  if (linkIncomplete.value) {
    return
  }

  linking.value = true
  linkFailure.value = null

  try {
    await addReference(props.documentId, {
      label: linkLabel.value,
      url: linkUrl.value,
      categoryId: linkCategory.value || undefined,
    })

    linkLabel.value = ''
    linkUrl.value = ''
    linkCategory.value = ''
    composingLink.value = false
    await load()
  } catch (error) {
    linkFailure.value = error instanceof ApiError ? error.message : 'The link could not be added.'
  } finally {
    linking.value = false
  }
}

/**
 * Saves the bytes.
 *
 * A blob and an object URL rather than a link to the route: the route needs a
 * bearer token and a browser navigation does not carry one. The URL is revoked
 * immediately — it exists for the length of one click.
 */
async function save(attachment: Attachment): Promise<void> {
  failure.value = null

  try {
    const blob = await downloadAttachment(props.documentId, attachment.id)
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')

    anchor.href = url
    anchor.download = attachment.originalFileName
    anchor.click()

    URL.revokeObjectURL(url)
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'The file could not be downloaded.'
  }
}

/** Removes a file or a link, after the second click. */
async function confirmDelete(id: string, kind: 'file' | 'link'): Promise<void> {
  deleteFailure.value = null

  try {
    if (kind === 'file') {
      await deleteAttachment(props.documentId, id)
    } else {
      await deleteReference(props.documentId, id)
    }

    confirming.value = null
    await load()
  } catch (error) {
    deleteFailure.value = error instanceof ApiError ? error.message : 'It could not be deleted.'
  }
}

/** Bytes, in the units the person chose the file in. */
function readableSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`
  }

  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function badgeVariant(attachment: Attachment): 'default' | 'secondary' | 'destructive' {
  if (attachment.virusScanStatus === 'CLEAN') {
    return 'default'
  }

  // **Both of the permanent refusals are destructive, and `PENDING` is not.**
  // The person waiting has nothing to do; the other two have to act.
  return attachment.virusScanStatus === 'PENDING' ? 'secondary' : 'destructive'
}

/**
 * How often the screen re-asks while a scan is outstanding, and how long it
 * keeps asking (**D-63**, [#326](https://github.com/sujanto-gaws/kelir/issues/326)).
 *
 * **Three seconds, under the worker's own five.** `attachment::worker` sweeps
 * for `PENDING` rows every `KELIR_ATTACHMENT_SCAN_INTERVAL` seconds — five by
 * default — so the answer appears at an arbitrary offset inside a five-second
 * window. Polling under that cadence means the screen is never a whole worker
 * cycle behind what the database already knows, and the request is the same
 * single list read the tab makes when it opens.
 *
 * **The ceiling is because a `PENDING` row is not always on its way to an
 * answer.** A scanner that cannot be reached leaves the file `PENDING`
 * indefinitely — `attachment_scan.rs` has a test named for exactly that — and a
 * screen with no cap would ask about it for as long as the tab stayed open. Two
 * minutes is far past the ~169 ms **D-4** measured for 25 MiB and short enough
 * that a broken scanner does not become a background request loop.
 *
 * **What happens after the ceiling is nothing, deliberately.** The badge still
 * reads `Checking`, which is still true — the row *is* pending. Turning it into
 * an error would be this screen inventing a verdict the server has not reached,
 * which is the mistake `readable_by` and `disclosable` both refuse one layer
 * down.
 */
const SCAN_POLL_MS = 3_000
const SCAN_POLL_LIMIT = 40

let scanPoll: ReturnType<typeof setInterval> | undefined
let scanPolls = 0

function scanOutstanding(): boolean {
  return attachments.value.some((attachment) => attachment.virusScanStatus === 'PENDING')
}

function stopWatchingScans(): void {
  if (scanPoll !== undefined) {
    clearInterval(scanPoll)
    scanPoll = undefined
  }
}

/**
 * Re-reads the list while something is being scanned.
 *
 * **The tab pays nothing when nothing is pending**, which is the ordinary case:
 * this starts only when a `PENDING` row exists and stops the moment none does.
 * Switching to another tab costs nothing either — the workspace mounts this
 * component under `v-if`, so leaving unmounts it and `onUnmounted` clears the
 * timer.
 *
 * **A hidden browser tab is skipped rather than counted.** Coming back to a
 * window left in the background should find the list fresh, not find the
 * ceiling already spent on requests nobody could see.
 */
function watchScans(): void {
  if (scanPoll !== undefined || !scanOutstanding()) {
    return
  }

  scanPolls = 0

  scanPoll = setInterval(() => {
    if (typeof document !== 'undefined' && document.hidden) {
      return
    }

    if (scanPolls >= SCAN_POLL_LIMIT) {
      stopWatchingScans()
      return
    }

    scanPolls += 1
    void load()
  }, SCAN_POLL_MS)
}

// The list is re-read whenever it changes, and the poll starts or stops on what
// it now holds. Both directions matter: an upload adds a `PENDING` row and
// starts it, and the scan clearing is what stops it.
watch(attachments, () => {
  if (scanOutstanding()) {
    watchScans()
  } else {
    stopWatchingScans()
  }
})

onUnmounted(stopWatchingScans)

watch(
  () => props.documentId,
  async () => {
    stopWatchingScans()
    await Promise.all([load(), loadCategories()])
  },
  { immediate: true },
)
</script>

<template>
  <div class="space-y-4" data-testid="attachments-tab">
    <p v-if="!canRead" class="text-sm text-muted-foreground" data-testid="attachments-forbidden">
      You do not have permission to see this document's attachments.
    </p>

    <template v-else>
      <div v-if="canUpload" class="space-y-2">
        <label class="text-sm font-medium" for="attachment-file">Attach a file</label>
        <input
          id="attachment-file"
          type="file"
          class="block text-sm"
          data-testid="attachment-input"
          :disabled="uploading"
          @change="upload"
        />
        <div v-if="categories.length > 0" class="flex items-center gap-2">
          <label class="text-xs text-muted-foreground" for="attachment-category">
            File it under
          </label>
          <select
            id="attachment-category"
            v-model="chosenCategory"
            class="rounded-md border p-1 text-sm"
            data-testid="attachment-category-picker"
          >
            <!-- **Uncategorized is an option, and it is the default.** A picker
                 with no way back to *not filed* makes the first careless choice
                 permanent. -->
            <option value="">Not filed</option>
            <option v-for="category in categories" :key="category.id" :value="category.id">
              {{ category.name }}
            </option>
          </select>
        </div>
        <p class="text-xs text-muted-foreground">
          A file is checked for viruses before it can be downloaded.
        </p>
      </div>

      <div v-if="canLink" class="space-y-2">
        <Button
          v-if="!composingLink"
          size="sm"
          variant="outline"
          data-testid="reference-open"
          @click="composingLink = true"
        >
          Add a link instead
        </Button>

        <div v-else class="space-y-2 rounded-md border p-3" data-testid="reference-form">
          <label class="text-sm font-medium" for="reference-label">What is it called</label>
          <input
            id="reference-label"
            v-model="linkLabel"
            type="text"
            class="w-full rounded-md border p-2 text-sm"
            data-testid="reference-label"
            :disabled="linking"
          />
          <label class="text-sm font-medium" for="reference-url">Where it is</label>
          <input
            id="reference-url"
            v-model="linkUrl"
            type="url"
            placeholder="https://"
            class="w-full rounded-md border p-2 text-sm"
            data-testid="reference-url"
            :disabled="linking"
          />
          <select
            v-if="categories.length > 0"
            v-model="linkCategory"
            class="rounded-md border p-1 text-sm"
            data-testid="reference-category-picker"
          >
            <option value="">Not filed</option>
            <option v-for="category in categories" :key="category.id" :value="category.id">
              {{ category.name }}
            </option>
          </select>

          <!-- The one thing a person adding a link should know before they do:
               nothing is copied here, and nothing is checked. -->
          <p class="text-xs text-muted-foreground" data-testid="reference-note">
            A link records where something is. Nothing is copied into this system and nothing is
            checked for viruses — whoever opens it goes to the other system.
          </p>

          <Alert v-if="linkFailure" variant="destructive" data-testid="reference-error">
            {{ linkFailure }}
          </Alert>

          <div class="flex gap-2">
            <Button
              size="sm"
              :disabled="linking || linkIncomplete"
              data-testid="reference-submit"
              @click="link"
            >
              Add link
            </Button>
            <Button
              size="sm"
              variant="ghost"
              data-testid="reference-cancel"
              @click="composingLink = false"
            >
              Cancel
            </Button>
          </div>
        </div>
      </div>

      <Alert v-if="uploadFailure" variant="destructive" data-testid="attachment-upload-error">
        {{ uploadFailure }}
      </Alert>

      <Alert v-if="failure" variant="destructive" data-testid="attachments-error">
        {{ failure }}
      </Alert>

      <Alert v-if="deleteFailure" variant="destructive" data-testid="attachment-delete-error">
        {{ deleteFailure }}
      </Alert>

      <p v-if="loading" class="text-sm text-muted-foreground">Loading attachments…</p>

      <p
        v-else-if="attachments.length === 0 && references.length === 0"
        class="text-sm text-muted-foreground"
        data-testid="attachments-empty"
      >
        Nothing is attached to this document yet.
      </p>

      <ul v-else class="space-y-3" data-testid="attachment-list">
        <li
          v-for="attachment in attachments"
          :key="attachment.id"
          class="rounded-md border p-3"
          data-testid="attachment-row"
        >
          <div class="flex flex-wrap items-center gap-2">
            <!-- Interpolated, never rendered as markup: the name is text the
                 uploader chose, and it is displayed rather than trusted. -->
            <span class="font-medium">{{ attachment.originalFileName }}</span>
            <Badge :variant="badgeVariant(attachment)" data-testid="attachment-status">
              {{ SCAN_STATUS_LABELS[attachment.virusScanStatus] }}
            </Badge>
            <Badge v-if="attachment.category" variant="outline" data-testid="attachment-category">
              {{ attachment.category.name }}
            </Badge>
            <span class="text-sm text-muted-foreground">
              {{ readableSize(attachment.fileSize) }}
            </span>
          </div>

          <p class="mt-1 text-sm text-muted-foreground" data-testid="attachment-explanation">
            {{ SCAN_STATUS_EXPLANATIONS[attachment.virusScanStatus] }}
          </p>

          <div class="mt-2 flex flex-wrap gap-2">
            <Button
              v-if="isDownloadable(attachment.virusScanStatus)"
              size="sm"
              variant="outline"
              data-testid="attachment-download"
              @click="save(attachment)"
            >
              Download
            </Button>

            <template v-if="mine(attachment) && canDelete">
              <Button
                v-if="confirming !== attachment.id"
                size="sm"
                variant="ghost"
                data-testid="attachment-delete"
                @click="confirming = attachment.id"
              >
                Delete
              </Button>
              <template v-else>
                <!-- **What *deleted* means here, said before it happens.** The
                     row goes and the stored copy is kept (D-52); a person who
                     expects shredding should not learn otherwise later. -->
                <span class="self-center text-xs text-muted-foreground" data-testid="delete-ask">
                  Remove this file from the document? The stored copy is kept.
                </span>
                <Button
                  size="sm"
                  variant="destructive"
                  data-testid="attachment-delete-confirm"
                  @click="confirmDelete(attachment.id, 'file')"
                >
                  Delete
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  data-testid="attachment-delete-cancel"
                  @click="confirming = null"
                >
                  Keep
                </Button>
              </template>
            </template>
          </div>
        </li>

        <!-- **A link, in the same list and never mistakable for a file.** No
             size, no scan badge, no download: it has none of those on the row
             and none on the type it is rendered from. -->
        <li
          v-for="reference in references"
          :key="reference.id"
          class="rounded-md border border-dashed p-3"
          data-testid="reference-row"
        >
          <div class="flex flex-wrap items-center gap-2">
            <span class="font-medium">{{ reference.label }}</span>
            <Badge variant="secondary" data-testid="reference-badge">Link</Badge>
            <Badge v-if="reference.category" variant="outline" data-testid="reference-category">
              {{ reference.category.name }}
            </Badge>
          </div>

          <p class="mt-1 break-all text-sm text-muted-foreground" data-testid="reference-url-text">
            {{ reference.url }}
          </p>

          <div class="mt-2 flex flex-wrap gap-2">
            <!-- `rel="noopener noreferrer"` because the target is somebody
                 else's page, and `safeHref` because an `href` is the one place a
                 stored string becomes an instruction. -->
            <a
              v-if="safeHref(reference.url)"
              :href="safeHref(reference.url)"
              target="_blank"
              rel="noopener noreferrer"
              class="text-sm underline"
              data-testid="reference-open-link"
            >
              Open
            </a>
            <span v-else class="text-sm text-destructive" data-testid="reference-unopenable">
              This link is not one this product will open.
            </span>

            <template v-if="mine(reference) && canDelete">
              <Button
                v-if="confirming !== reference.id"
                size="sm"
                variant="ghost"
                data-testid="reference-delete"
                @click="confirming = reference.id"
              >
                Remove
              </Button>
              <template v-else>
                <span class="self-center text-xs text-muted-foreground" data-testid="delete-ask">
                  Remove this link?
                </span>
                <Button
                  size="sm"
                  variant="destructive"
                  data-testid="reference-delete-confirm"
                  @click="confirmDelete(reference.id, 'link')"
                >
                  Remove
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  data-testid="reference-delete-cancel"
                  @click="confirming = null"
                >
                  Keep
                </Button>
              </template>
            </template>
          </div>
        </li>
      </ul>
    </template>
  </div>
</template>
