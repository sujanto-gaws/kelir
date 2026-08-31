import { apiClient, getPage, postItem, toApiError } from './client'
import type { Page } from '@/types/api'
import type { Attachment } from '@/types/attachment'

/**
 * A document's attachments (`/api/v1/documents/{id}/attachments`).
 *
 * Thin, like `documents.ts` and `tasks.ts` — except for the two calls that
 * cannot be, and each says why.
 */

/** A document's attachments, newest first. */
export function listAttachments(documentId: string, page = 1): Promise<Page<Attachment>> {
  return getPage<Attachment>(`/documents/${documentId}/attachments`, { page })
}

/**
 * Attaches a file.
 *
 * **`FormData` rather than JSON**, which is the one place this client's
 * envelope helpers meet a request they did not anticipate. `postItem` still
 * unwraps the reply — the response is the ordinary item envelope — and axios
 * sets the multipart boundary itself, which is why no `Content-Type` is passed:
 * setting one by hand omits the boundary and the server cannot parse the body.
 */
export function uploadAttachment(
  documentId: string,
  file: File,
  description?: string,
): Promise<Attachment> {
  const body = new FormData()

  body.append('file', file)

  if (description && description.trim().length > 0) {
    body.append('description', description.trim())
  }

  return postItem<Attachment>(`/documents/${documentId}/attachments`, body)
}

/**
 * Fetches the bytes.
 *
 * **Not an `<a href>`**, and that is the reason this function exists. The route
 * is behind a bearer token, which a plain link cannot carry; a browser
 * navigation would arrive unauthenticated and answer 401. So the bytes come
 * through the same client every other call uses, as a blob, and the caller
 * turns that into a save.
 *
 * The file name comes from the row the caller already holds rather than from
 * `Content-Disposition`. The header is set — deliberately, and always
 * `attachment` — but parsing it here would be a second source for a name this
 * screen has in hand.
 */
export async function downloadAttachment(documentId: string, attachmentId: string): Promise<Blob> {
  try {
    const response = await apiClient.get<Blob>(
      `/documents/${documentId}/attachments/${attachmentId}`,
      { responseType: 'blob' },
    )

    return response.data
  } catch (error) {
    throw toApiError(error)
  }
}
