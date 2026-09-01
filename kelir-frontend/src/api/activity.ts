import { getPage } from './client'
import type { ActivityEvent } from '@/types/activity'
import type { Page } from '@/types/api'

/**
 * A document's activity timeline (`/api/v1/documents/{id}/activity`).
 *
 * Thin, like `comments.ts` — one call and an ordinary envelope.
 */

/**
 * One page of a document's timeline, **newest first**.
 *
 * **Read through the document's own permission and no other** (**D-47**, from
 * [#250](https://github.com/sujanto-gaws/kelir/issues/250) AC2). There is no
 * `activity:read` check to mirror here: whether this caller may see what
 * happened to a document is the same question as whether they may see the
 * document, and the server asks it once.
 *
 * That is why this file has no `can(…)` guard and `comments.ts`'s caller does.
 */
export function listActivity(
  documentId: string,
  page = 1,
  pageSize = 20,
): Promise<Page<ActivityEvent>> {
  return getPage<ActivityEvent>(`/documents/${documentId}/activity`, { page, pageSize })
}
