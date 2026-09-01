import { deleteItem, getPage, postItem, putItem } from './client'
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

/**
 * Adds a comment to a document, or a reply to one of its comments.
 *
 * **A reply is the same call.** `parentCommentId` names the comment being
 * answered and is omitted for a root — there is no second endpoint, because with
 * one level of threading there is nothing different about writing a reply.
 */
export function addComment(
  documentId: string,
  body: string,
  parentCommentId?: string,
): Promise<Comment> {
  return postItem<Comment>(`/documents/${documentId}/comments`, {
    body,
    ...(parentCommentId === undefined ? {} : { parentCommentId }),
  })
}

/**
 * Replaces what a comment says.
 *
 * **PUT, not PATCH**, which is this product's update verb everywhere: a
 * comment's whole representation is its body, so there is nothing for a partial
 * update to be partial about. The server refuses anything but the author.
 */
export function editComment(documentId: string, commentId: string, body: string): Promise<Comment> {
  return putItem<Comment>(`/documents/${documentId}/comments/${commentId}`, { body })
}

/**
 * Deletes a comment — softly, and **the reply under it survives**.
 *
 * A deleted comment that has been replied to stays in the list with no body,
 * which is why the caller reloads rather than splicing the row out: whether it
 * disappears or becomes a tombstone is the server's answer, not the screen's
 * guess.
 */
export function deleteComment(documentId: string, commentId: string): Promise<void> {
  return deleteItem(`/documents/${documentId}/comments/${commentId}`)
}
