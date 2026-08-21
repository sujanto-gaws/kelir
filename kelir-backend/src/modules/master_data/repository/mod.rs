//! SQLx access for the master-data module. Private to the module: other modules
//! go through `service` (coding standard §2.2).
//!
//! Every query filters by `tenant_id` (Database Schema §1.5) and excludes
//! soft-deleted rows.
//!
//! **Decimal columns travel as text.** `sqlx` is built here without
//! `bigdecimal` or `rust_decimal`, so `query!` cannot decode `NUMERIC` at all.
//! `annual_revenue` is therefore selected as `::text` and bound as
//! `($n::text)::numeric` — the inner cast is what tells the macro the parameter
//! is text, which a bare `CAST($n AS NUMERIC)` does not. That is not only a
//! workaround: a JSON number is an IEEE double and `NUMERIC(18,2)` has values it
//! cannot hold exactly, so the string is the honest wire shape either way.

pub mod party;
pub mod role;
pub mod role_view;

// Re-exported flat, so the service keeps addressing these as `repo::find_party`
// — which file a query lives in is a question about this module's size, not
// about its interface.
pub use party::*;
pub use role::*;
pub use role_view::*;
