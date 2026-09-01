import { defineStore } from 'pinia'
import { ref } from 'vue'

import { unreadCount } from '@/api/notifications'
import { useAuthStore } from '@/stores/auth'

/**
 * The unread badge (FR-NTF-003; [#251]).
 *
 * # A number, and deliberately nothing else
 *
 * This store holds no notifications. The centre owns its own page — it is the
 * only screen that needs the rows, and caching them here would mean two places
 * that can disagree about whether something has been read.
 *
 * **Refreshed on demand rather than polled.** A poll is a request per user per
 * interval for a number that is usually unchanged, and the moment it exists
 * somebody has to choose the interval. What this does instead is refresh when
 * the shell mounts and after an action that could have changed it, which is
 * every point a person would notice. **A notification that arrives while a
 * screen is open is seen on the next navigation**, and that is a stated limit
 * rather than an oversight: real-time delivery is a channel (FR-NTF-004,
 * [#257]) and not a shorter timer.
 *
 * [#251]: https://github.com/sujanto-gaws/kelir/issues/251
 * [#257]: https://github.com/sujanto-gaws/kelir/issues/257
 */
export const useNotificationStore = defineStore('notifications', () => {
  const unread = ref(0)

  /**
   * Re-reads the count, **silently**.
   *
   * A badge that cannot be read is a badge that shows nothing; there is no
   * error state here because there is no message worth interrupting somebody
   * with. The centre reports its own failures, where a person went looking.
   */
  async function refresh(): Promise<void> {
    const auth = useAuthStore()

    // Asked for by the shell on every mount, including before sign-in and for
    // accounts that hold no notification permission — where the request would
    // be a guaranteed 401 or 403.
    if (!auth.isAuthenticated || !auth.can('notification:read')) {
      unread.value = 0
      return
    }

    try {
      unread.value = (await unreadCount()).unread
    } catch {
      // Leave the last known number rather than blanking the badge: a failed
      // refresh is not evidence that nothing is waiting.
    }
  }

  /** What the server said is left, after a bulk clear. */
  function set(value: number): void {
    unread.value = Math.max(0, value)
  }

  /**
   * One fewer, for a row the centre has just marked read.
   *
   * Kept non-negative rather than trusted: this is an optimistic edit made
   * beside a request that can fail, and the caller puts the row back and calls
   * [`refresh`] when it does.
   */
  function decrement(): void {
    unread.value = Math.max(0, unread.value - 1)
  }

  return { unread, refresh, set, decrement }
})
