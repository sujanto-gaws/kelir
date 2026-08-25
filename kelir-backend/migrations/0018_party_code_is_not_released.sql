-- 0018_party_code_is_not_released.sql — deleting a party no longer frees its
-- `partyId` for another party to take (#107).
--
-- `uq_mdm_parties_tenant_id_party_code` was partial on `deleted_at IS NULL`, so
-- a soft-deleted party released its code. Meanwhile every read path stores a
-- *UUID* and renders it as whatever code that UUID's row currently carries. The
-- two together re-target stored references without touching them:
--
--   1. Customer `PARTY-CUST` has `billingPartyId: "PARTY-BILL"`.
--   2. `PARTY-BILL` is deleted.
--   3. An unrelated party is created and takes the freed code `PARTY-BILL`.
--   4. `GET /parties/{cust}/roles` still answers `"billingPartyId": "PARTY-BILL"`
--      — a code that now names a different legal entity.
--
-- A consumer resolving that code through `find_party_id_by_code` lands on the
-- wrong party. `relationshipsFrom`/`relationshipsTo`, `managerPartyId` and
-- `assistantPartyId` have the same shape.
--
-- **Of the three ways out, this is the first**: do not release the code. It is
-- the one that removes the failure class instead of reporting it, and it is
-- truest to what a business identifier is — `partyId` names one thing, for as
-- long as the deployment remembers that thing at all. The cost is that a
-- mistyped code is unusable until someone hard-deletes the row, which is a
-- support question rather than a correctness one.
--
-- The two rejected alternatives, so this is not re-litigated from scratch:
-- rendering a soft-deleted referent as `null` would report a dangling reference
-- honestly but leaves the code free to be re-taken, so the *live* references
-- still change meaning; and documenting the behaviour as intended would leave
-- every consumer of a party code responsible for knowing that a code is only
-- valid as of the moment it was read.
--
-- The index keeps its name, so `duplicate_party_to_conflict` still maps a
-- violation onto a 409 rather than a 500.
--
-- **N−1 compatibility — schema half.** No column changes, nothing is dropped or
-- renamed, and every query the previous release holds still type-checks. The
-- previous release keeps running against this schema with one behavioural
-- difference, and it is the point of the migration: creating a party whose code
-- a deleted party still holds is answered `409 Conflict` where it used to be
-- `201 Created`. That is a refusal, not a corruption, and it is the refusal the
-- new schema exists to give.
--
-- **The guard below is deliberate and is not a no-op on a fresh database.** A
-- deployment that has already exercised the defect holds a live party and a
-- deleted one sharing a code, and `CREATE UNIQUE INDEX` over those rows fails
-- with `could not create unique index` and a duplicate key — accurate and
-- unactionable. Raising first names every offending pair, so the operator knows
-- what to reconcile before retrying. A deployment that has not exercised the
-- defect sees nothing.
DO $$
DECLARE
    offenders TEXT;
BEGIN
    SELECT string_agg(format('%s / %s (%s rows)', tenant_id, party_code, n), '; ')
      INTO offenders
      FROM (
          SELECT tenant_id, party_code, count(*) AS n
            FROM mdm_parties
           GROUP BY tenant_id, party_code
          HAVING count(*) > 1
      ) duplicates;

    IF offenders IS NOT NULL THEN
        RAISE EXCEPTION
            'issue #107: a party code is held by more than one party, including deleted ones: %',
            offenders
        USING HINT = 'reconcile or hard-delete the duplicates, then re-run this migration';
    END IF;
END
$$;

DROP INDEX uq_mdm_parties_tenant_id_party_code;

CREATE UNIQUE INDEX uq_mdm_parties_tenant_id_party_code
    ON mdm_parties (tenant_id, party_code);
