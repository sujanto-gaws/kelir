-- 0024_one_live_role_per_party.sql — a party holds a role once, and the
-- database is what says so (#115).
--
-- `uq_mdm_party_roles_party_id_role_type_id_starts_at` included `starts_at`, so
-- two live rows for the same party and the same role type were legal as long as
-- their `fromDate`s differed. That is what let #105 happen: `assign_role`
-- checked for an existing assignment and the database did not, so two
-- concurrent requests both read *no such role* and both inserted. It left the
-- party holding one role twice, 28 times in 30.
--
-- #105 closed the race by locking the party row for the transaction that
-- writes, which made the invariant the service's to keep. This makes it the
-- database's as well. Both stay: the index catches what a future writer
-- forgets, and the lock keeps the outcome a correct 200/201 rather than the 500
-- a unique violation would surface as.
--
-- **`starts_at` was carrying nothing.** The reason to suspect otherwise was a
-- party that was a supplier, stopped, and started again — two periods, and an
-- index that could not tell them apart. It can: `soft_delete_party_role` sets
-- `deleted_at` as well as `ends_at`, so the earlier period is invisible to a
-- partial index on `deleted_at IS NULL` and cannot collide with the later one.
-- The column stays on the table, where it is the assignment's period and is
-- read as such; it comes out of the key only.
--
-- The key was never a decision. It arrived with the original document set,
-- ahead of `0008_master_data.sql` and of any code, and §4.5 recorded it with no
-- rationale — the Party-model temporal-key idiom applied by habit. §14 carries
-- that as a deviation rather than leaving this file as the only account of it.
--
-- **The index is not scoped to the tenant, deliberately.** `party_id` already
-- names one tenant's party — `mdm_party_roles.party_id` references
-- `mdm_parties`, which carries `tenant_id` — so a tenant column in the key
-- would widen it to no purpose and would let one tenant's role row sit beside
-- another's on the same party. That state is unreachable through the API and is
-- now unconstructible in the database, which is a stronger statement than the
-- cross-tenant tests could previously make: they built it directly and asserted
-- that every read ignored it.
--
-- **N−1 compatibility — schema half.** No column changes, nothing dropped or
-- renamed, and every query the previous release holds still type-checks; the
-- old index served `find_live_party_role` on a `(party_id, role_type_id)`
-- prefix and the new one serves it directly. `v0.4.0`'s only writer into this
-- table is `assign_role`, which holds the party row under `FOR UPDATE` and
-- updates in place, so the previous binary cannot reach the new violation
-- through the API. Note the shape of that argument: release process §6 is
-- written about *adding* columns and does not cover *tightening* a constraint,
-- so the compatibility here rests on the old binary's single writer already
-- respecting the invariant, not on the change being additive.
--
-- **The guard below refuses rather than repairs, and that is the rule.** Where
-- `0018` could not choose which party keeps a code, this cannot choose which
-- assignment is the real one: two live rows differ in `starts_at`, `comments`
-- and `attributes_json`, and the profile behind them is shared, so closing
-- either one discards the only record of what the duplicate was. A migration
-- that picked silently would destroy evidence of a defect in the course of
-- enforcing against it. It names every offending pair instead and stops.
--
-- The exposure is small and worth stating so the guard is not read as alarm:
-- `mdm_party_roles` first exists in `0008`, which ships in `v0.3.0`, and
-- `v0.3.0` already carries #105's lock. **No released binary has ever been able
-- to write a duplicate live row.** What remains is a developer database created
-- between #91 and #105 — Testcontainers start empty, so this never fires in CI.
DO $$
DECLARE
    offenders TEXT;
BEGIN
    SELECT string_agg(format('party %s / role type %s (%s live rows)', party_id, role_type_id, n), '; ')
      INTO offenders
      FROM (
          SELECT party_id, role_type_id, count(*) AS n
            FROM mdm_party_roles
           WHERE deleted_at IS NULL
           GROUP BY party_id, role_type_id
          HAVING count(*) > 1
      ) duplicates;

    IF offenders IS NOT NULL THEN
        RAISE EXCEPTION
            'issue #115: a party holds the same role type more than once: %',
            offenders
        USING HINT = 'close the assignments that are not current — set deleted_at and ends_at, as remove_role does — then re-run this migration';
    END IF;
END
$$;

DROP INDEX uq_mdm_party_roles_party_id_role_type_id_starts_at;

CREATE UNIQUE INDEX uq_mdm_party_roles_party_id_role_type_id
    ON mdm_party_roles (party_id, role_type_id) WHERE deleted_at IS NULL;
