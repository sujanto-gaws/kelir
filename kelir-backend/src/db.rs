// Placeholder for the database layer. Phase 1 replaces this with the SQLx
// PgPool, the migration runner and the readiness check; `sqlx` is added as a
// dependency in that change rather than sitting unused until then.
#[allow(dead_code, reason = "scaffold consumed by the Phase 1 SQLx pool")]
#[derive(Debug, Clone, Default)]
pub struct DatabaseState;
