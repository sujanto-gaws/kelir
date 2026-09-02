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
  /**
   * What the comment says, or **null on a tombstone** — a deleted comment the
   * list still carries because replies hang from it. The body is withheld by the
   * server, not blank: a comment of nothing is a thing the API refuses to store.
   */
  body: string | null
  /**
   * The comment this one replies to, null on a root. Threading is **one level**,
   * so a comment with this set has no replies of its own.
   */
  parentCommentId: string | null
  authorUserId: string | null
  /**
   * The author's name **now**, joined rather than denormalized — a conversation
   * has current participants, where `activity_events.actorName` keeps the name
   * somebody had when a thing happened because a history has the people who
   * were there.
   */
  authorUsername: string | null
  createdAt: string
  /**
   * When the body last changed, null on a comment nobody has edited.
   *
   * **Not `updatedAt`**, which the server moves for any write to the row, the
   * delete included. An edit has to be visible *as* an edit — a comment whose
   * text changed with nothing saying so is a conversation somebody can rewrite
   * after the fact — and this is the field that says so.
   */
  editedAt: string | null
  /** When it was deleted. Non-null only on a tombstone, whose `body` is null. */
  deletedAt: string | null
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
