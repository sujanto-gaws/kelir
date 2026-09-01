/**
 * The activity timeline, as the API reports it
 * (`/api/v1/documents/{id}/activity`).
 *
 * # This is not the audit trail, and this file is the first place a reader
 * finds that out
 *
 * Kelir keeps four records of what happened, and the two most easily merged are
 * `activity_events` and `audit_events`. **This is the first.** It answers *what
 * happened to this document*, it is read by whoever is looking at the document,
 * and it is written in the same transaction as the action — so an approval that
 * rolled back leaves no entry saying it happened.
 *
 * The audit trail answers *was this tampered with*, is hash-chained, sits
 * behind its own permission and is written on its own connection so that it
 * survives the action failing. Neither record is derived from the other, and
 * `modules::activity`'s header carries the full argument.
 *
 * # The subject is an id, never a description
 *
 * `details` says what happened to the **document** and nothing about the thing
 * an entry is about: an attachment's name, a comment's length and the second
 * party to a delegation are behind `attachment:read`, `comment:read` and the
 * workflow's own read, and a timeline repeating them would be answering three
 * other surfaces' questions without asking their permissions (**D-45**, from
 * [#292](https://github.com/sujanto-gaws/kelir/issues/292)).
 *
 * So `attachmentId`, `commentId`, `taskId` and `workflowInstanceId` are what an
 * entry carries instead, and a screen that wants the name goes and asks the
 * surface that guards it.
 */

/** Which part of the product an event came from. */
export type EventCategory =
  | 'DOCUMENT'
  | 'ATTACHMENT'
  | 'COMMENT'
  | 'WORKFLOW'
  | 'SECURITY'
  | 'MASTER_DATA'
  | 'NOTIFICATION'

export interface ActivityEvent {
  id: string
  documentId: string | null

  /** The four links. Present when the entry is about one of these. */
  workflowInstanceId: string | null
  taskId: string | null
  attachmentId: string | null
  commentId: string | null

  /** The dotted vocabulary of naming convention §7 — `Document.Submitted`. */
  eventType: string
  eventCategory: EventCategory

  actorUserId: string | null
  /**
   * The actor's name **when this happened**, denormalized at write time rather
   * than joined now — so a rename or a removal does not rewrite the past. It is
   * the opposite choice from `Comment.authorUsername`, and deliberately: a
   * conversation has current participants, a history has the people who were
   * there.
   */
  actorName: string | null

  /** The server's own sentence for the entry. */
  actionSummary: string

  /** What happened to the document, and nothing about the subject. */
  details: Record<string, unknown>

  occurredAt: string
}

/**
 * How a category is labelled, so the reader can tell the four sources apart.
 *
 * **Every category the API can return has a label**, including the three no
 * writer produces yet. `EventCategory::from_db` falls back to `DOCUMENT` for a
 * value it does not know, so an unlabelled one cannot reach here — but a later
 * release adding a writer would otherwise render a blank chip, and a blank chip
 * on a timeline reads as *this entry is broken* rather than as *this is new*.
 */
export const EVENT_CATEGORY_LABELS: Record<EventCategory, string> = {
  DOCUMENT: 'Document',
  ATTACHMENT: 'Attachment',
  COMMENT: 'Comment',
  WORKFLOW: 'Workflow',
  SECURITY: 'Security',
  MASTER_DATA: 'Master data',
  NOTIFICATION: 'Notification',
}
