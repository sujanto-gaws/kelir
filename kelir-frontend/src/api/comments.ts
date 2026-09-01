import { getPage, postItem } from './client'
import type { Page } from '@/types/api'
import type { Comment } from '@/types/comment'

/**
 * A document's conversation (`/api/v1/documents/{id}/comments`).
 *
 * Thin, like `documents.ts` — both calls are ordinary envelopes, which is the
 * difference from `attachments.ts`, where a multipart upload and a blob
 * download each needed a paragraph.
 */

/**
 * A document's comments, **oldest first**.
 *
 * The order is the server's and it is the opposite of every other list in this
 * product: a conversation is read in the order it was said, where a list of
 * records is read newest-first because the newest is the one you came for.
 */
export function listComments(documentId: string, page = 1): Promise<Page<Comment>> {
  return getPage<Comment>(`/documents/${documentId}/comments`, { page })
}

/** Adds a comment to a document. */
export function addComment(documentId: string, body: string): Promise<Comment> {
  return postItem<Comment>(`/documents/${documentId}/comments`, { body })
}
