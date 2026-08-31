<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { downloadAttachment, listAttachments, uploadAttachment } from '@/api/attachments'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import {
  isDownloadable,
  SCAN_STATUS_EXPLANATIONS,
  SCAN_STATUS_LABELS,
  type Attachment,
} from '@/types/attachment'

const props = defineProps<{ documentId: string }>()
const auth = useAuthStore()

/**
 * The Attachments tab of the document workspace (FR-ATT-001..003; [#295]).
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
 * # What the browser does not decide
 *
 * **The download button follows the server's status and nothing else.** There
 * is no client-side rule about which files are safe; `isDownloadable` reads one
 * field, and the API refuses anything else with a 409 whatever this screen
 * renders. That is deliberate: the gate is enforced where the bytes are served
 * ([#246] AC4), and a screen is a convenience over it rather than a second
 * copy of it.
 *
 * **And the two refusals at the door are the server's words.** A size limit and
 * an allowed-type list live in configuration; re-deriving either here would be
 * a second policy that drifts from the one that decides.
 *
 * [#295]: https://github.com/sujanto-gaws/kelir/issues/295
 * [#246]: https://github.com/sujanto-gaws/kelir/issues/246
 */

const attachments = ref<Attachment[]>([])
const loading = ref(false)
const failure = ref<string | null>(null)
const uploadFailure = ref<string | null>(null)
const uploading = ref(false)
const fileInput = ref<HTMLInputElement | null>(null)

const canRead = computed(() => auth.can('attachment:read'))
const canUpload = computed(() => auth.can('attachment:create'))

async function load(): Promise<void> {
  if (!canRead.value) {
    return
  }

  loading.value = true
  failure.value = null

  try {
    const page = await listAttachments(props.documentId)
    attachments.value = page.items
  } catch (error) {
    failure.value =
      error instanceof ApiError ? error.message : 'The attachments could not be loaded.'
  } finally {
    loading.value = false
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
    await uploadAttachment(props.documentId, file)
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
    const link = document.createElement('a')

    link.href = url
    link.download = attachment.originalFileName
    link.click()

    URL.revokeObjectURL(url)
  } catch (error) {
    failure.value = error instanceof ApiError ? error.message : 'The file could not be downloaded.'
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

watch(() => props.documentId, load, { immediate: true })
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
          ref="fileInput"
          type="file"
          class="block text-sm"
          data-testid="attachment-input"
          :disabled="uploading"
          @change="upload"
        />
        <p class="text-xs text-muted-foreground">
          A file is checked for viruses before it can be downloaded.
        </p>
      </div>

      <Alert v-if="uploadFailure" variant="destructive" data-testid="attachment-upload-error">
        {{ uploadFailure }}
      </Alert>

      <Alert v-if="failure" variant="destructive" data-testid="attachments-error">
        {{ failure }}
      </Alert>

      <p v-if="loading" class="text-sm text-muted-foreground">Loading attachments…</p>

      <p
        v-else-if="attachments.length === 0"
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
            <span class="text-sm text-muted-foreground">
              {{ readableSize(attachment.fileSize) }}
            </span>
          </div>

          <p class="mt-1 text-sm text-muted-foreground" data-testid="attachment-explanation">
            {{ SCAN_STATUS_EXPLANATIONS[attachment.virusScanStatus] }}
          </p>

          <div class="mt-2">
            <Button
              v-if="isDownloadable(attachment.virusScanStatus)"
              size="sm"
              variant="outline"
              data-testid="attachment-download"
              @click="save(attachment)"
            >
              Download
            </Button>
          </div>
        </li>
      </ul>
    </template>
  </div>
</template>
