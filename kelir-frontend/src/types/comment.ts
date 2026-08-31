/**
 * Comments, as the API reports them (`/api/v1/documents/{id}/comments`).
 *
 * **This is not the decision comment**, and the distinction is the one thing a
 * screen over these rows most easily loses. FR-TASK-006 shipped in Sprint 11 as
 * three columns written with an approval and immutable because the decision is;
 * these are a conversation, which a later sprint lets people reply to, edit and
 * resolve. `modules::comment`'s own documentation carries the full argument.
 */

export interface Comment {
  id: string
  documentId: string
  body: string
  authorUserId: string | null
  /**
   * The author's name **now**, joined rather than denormalized — a conversation
   * has current participants, where `activity_events.actorName` keeps the name
   * somebody had when a thing happened because a history has the people who
   * were there.
   */
  authorUsername: string | null
  createdAt: string
}

/**
 * The longest body the API will store, mirrored so the screen can say so before
 * a round trip.
 *
 * **The server still decides.** This is a courtesy that saves a refused request,
 * not a second policy: `comment::domain::MAX_COMMENT_BODY` is the bound that
 * holds, and a mismatch here costs a redundant 422 rather than an accepted
 * comment nobody stored.
 */
export const MAX_COMMENT_BODY = 4000
