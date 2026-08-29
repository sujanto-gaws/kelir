-- 0029_workflow_routing.sql — why a process went one way and not the other
-- (FR-WF-015, #186 AC5).
--
-- **One nullable column, and the question it answers is not "where did this
-- go".** `workflow_history` has said that since `0027`: `from_state`,
-- `to_state`, and the action that moved it. What it could not say is *why that
-- branch* — and a definition whose transitions are picked by JSON Logic makes
-- that the question somebody actually asks. #186 AC5's words are "why did this
-- go to her and not to him", and neither the target nor the action answers it.
--
-- **It records the evaluation, not the conclusion.** The chosen edge's own
-- condition is a tautology on a history row — it is the one that held, or the
-- fallback that needed nothing — so storing only that would answer half the
-- question and look like all of it. What goes in is every condition the engine
-- actually evaluated, in the order S7 puts them, each with the boolean it
-- produced:
--
--     [{"to": "DIRECTOR_APPROVAL", "condition": {...}, "outcome": false},
--      {"to": "FINANCE_APPROVAL",  "condition": {...}, "outcome": true}]
--
-- The rows the engine never reached are absent rather than recorded as false,
-- because it stops at the first condition that holds and saying otherwise would
-- be a record of something that did not happen.
--
-- **`JSONB` and not a table.** A routing trail is read with the history row it
-- belongs to, never queried across rows, and never joined — the three things
-- that would argue for one. A `workflow_history_conditions` table would be a
-- second row to keep in step with the first inside the transition's
-- transaction, for a payload bounded by the definition's own size.
--
-- **`NULL` where nothing was evaluated**, which is most rows: an instance's
-- first state, and every transition whose action leaves exactly one
-- unconditioned edge. An empty array would say the engine evaluated nothing on
-- a path where it had nothing to evaluate — true but noisy, and indistinguishable
-- from a trail that was lost.
--
-- **Append-only, like the table it joins.** No `deleted_at`, no `updated_at`;
-- `0027`'s header carries the reasoning, and adding a column does not reopen it.
--
-- **N−1 compatibility — schema half.** One nullable column added, nothing
-- altered and nothing dropped. The previous release's binary names its columns
-- explicitly in both the insert and the select on this table, so it neither
-- writes nor reads this one and starts against this schema unchanged.

ALTER TABLE workflow_history
    ADD COLUMN routing_json JSONB;

COMMENT ON COLUMN workflow_history.routing_json IS
    'Every transition condition the engine evaluated to choose this edge, in S7 order, each with its outcome (#186 AC5). Null where nothing was evaluated.';
