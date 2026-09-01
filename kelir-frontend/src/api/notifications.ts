import { getItem, getPage, postItem, postVoid } from './client'
import type { Page } from '@/types/api'
import type { AppNotification, UnreadCount } from '@/types/notification'

/**
 * The caller's own notifications (`/api/v1/notifications`).
 *
 * **No subject in any path.** The token says whose these are, which is why
 * there is no id to pass and no id somebody could pass a different value for.
 */

/** One page of the caller's notifications, newest first. */
export function listNotifications(page = 1, pageSize = 20): Promise<Page<AppNotification>> {
  return getPage<AppNotification>('/notifications', { page, pageSize })
}

/**
 * How many are waiting.
 *
 * **Its own call rather than a field on the list**, because the badge is wanted
 * far more often than the page and by a caller that needs neither rows nor a
 * page's worth of work to get one number.
 */
export function unreadCount(): Promise<UnreadCount> {
  return getItem<UnreadCount>('/notifications/unread-count')
}

/**
 * Marks one read. **Idempotent** — calling it again is the same 204, so a retry
 * costs nothing and does not move the timestamp.
 */
export function markNotificationRead(id: string): Promise<void> {
  // `postVoid`, because this answers 204 with no envelope for `unwrapItem` to
  // find — the shape sign-out and change-password already have.
  return postVoid(`/notifications/${id}/read`)
}

/** Clears the badge; answers with what is left rather than what was cleared. */
export function markAllNotificationsRead(): Promise<UnreadCount> {
  return postItem<UnreadCount>('/notifications/read')
}
