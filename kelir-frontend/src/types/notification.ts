/**
 * Notifications, as the API reports them (`/api/v1/notifications`).
 *
 * **Addressed to you, not attached to a record** — which is why this collection
 * has no subject in its path where comments and attachments hang off a
 * document. Every row the API can return is the caller's own, so there is no
 * `recipientUserId` here: it would be telling somebody their own id back.
 */

/** What a notification is about. `OTHER` is a type this build does not know. */
export type NotificationType = 'TASK_ASSIGNED' | 'DOCUMENT_DECIDED' | 'OTHER'

export interface AppNotification {
  id: string

  /** Where it points; a client offers the document, the task, or neither. */
  documentId: string | null
  workflowInstanceId: string | null
  taskId: string | null

  notificationType: NotificationType
  title: string
  body: string

  /** Null while unread — the only place readness lives. */
  readAt: string | null
  createdAt: string
}

export interface UnreadCount {
  unread: number
}

/**
 * How a type is labelled.
 *
 * **`OTHER` has a label too.** A row written by a later release comes back as
 * `OTHER` rather than being refused, and a blank chip beside it would read as
 * *this notification is broken* rather than as *this is new*. Its `title` and
 * `body` are the row's own words and carry the meaning regardless.
 */
export const NOTIFICATION_TYPE_LABELS: Record<NotificationType, string> = {
  TASK_ASSIGNED: 'Task',
  DOCUMENT_DECIDED: 'Decision',
  OTHER: 'Notification',
}
