//! SQLx access for the master-data module. Private to the module: other modules
//! go through `service` (coding standard §2.2).
//!
//! **Every query filters by `tenant_id` (Database Schema §1.5) and excludes
//! soft-deleted rows — on the base table *and on every join over a
//! tenant-scoped table*.** The second half is what #108 was about: the sentence
//! was here, a reader auditing this module believed it, and it was not true.
//! `list_relationships` and `list_party_roles` filtered their base table and
//! joined `mdm_parties` and `mdm_role_types` with no tenant predicate, so a
//! cross-tenant row present in storage would have rendered another tenant's
//! `party_code` inside `GET /parties/{mine}`. `list_statuses`,
//! `list_contact_mechs`, `list_parties` and the three profile joins had the same
//! shape.
//!
//! It was latent rather than exploitable: `resolve_relationships` and
//! `resolve_party_reference` both resolve through the tenant-scoped
//! `find_party_id_by_code`, so the API could not create such a row. One bulk
//! import or one admin script is all that stood between latent and live, which
//! is why the fix is the predicate rather than a note about the write paths.
//!
//! The exceptions, stated rather than left to be rediscovered:
//!
//! * `find_party_role_by_id` was the module's one unscoped read and is gone —
//!   #119 removed the read-back it existed for.
//! * `update_party_role` takes `tenant_id` and `party_id` as **defence in
//!   depth**. Its `id` is already produced by a scoped read under the party's
//!   lock, so the predicate is not what makes the write safe today; it is what
//!   makes the sentence above true of every statement here, with no statement
//!   that would obey any id it was handed.
//! * `lock_facility_hierarchy` names no table. It is tenant-scoped all the same
//!   — the lock is keyed on a hash of the tenant — and that is the whole point
//!   of it (#133).
//! * "Excludes soft-deleted rows" reads *where the table has that column*.
//!   `mdm_party_statuses` and `audit_events` are append-only histories with no
//!   `deleted_at` at all; a predicate there would not compile, let alone hold.
//! * `mdm_persons`' tenant predicate in `list_parties` cannot be reached: the
//!   extension tables are unique on `party_id` across tenants, so a foreign
//!   extension row can exist only where the party has none, and such a party is
//!   a `PARTY_GROUP` whose `group_name` wins the name's `COALESCE` ahead of any
//!   person. It is written anyway — the two joins are one statement, and which
//!   half is load-bearing should not be something a reader has to derive.
//!
//! **Decimal columns travel as text.** `sqlx` is built here without
//! `bigdecimal` or `rust_decimal`, so `query!` cannot decode `NUMERIC` at all.
//! `annual_revenue` is therefore selected as `::text` and bound as
//! `($n::text)::numeric` — the inner cast is what tells the macro the parameter
//! is text, which a bare `CAST($n AS NUMERIC)` does not. That is not only a
//! workaround: a JSON number is an IEEE double and `NUMERIC(18,2)` has values it
//! cannot hold exactly, so the string is the honest wire shape either way.

pub mod audit_record;
pub mod facility;
pub mod party;
pub mod party_children;
pub mod record_status;
pub mod role;
pub mod role_view;

// Re-exported flat, so the service keeps addressing these as `repo::find_party`
// — which file a query lives in is a question about this module's size, not
// about its interface.
pub use audit_record::*;
pub use facility::*;
pub use party::*;
pub use party_children::*;
pub use record_status::*;
pub use role::*;
pub use role_view::*;
