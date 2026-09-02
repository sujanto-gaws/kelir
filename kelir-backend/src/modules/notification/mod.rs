//! Notifications — telling somebody a thing is waiting for them
//! (SRS §4.13, FR-NTF-001/002/003; [#251]).
//!
//! # The rule this module is built on is the one `modules::activity` earned
//!
//! **A notification is written in the transaction of the thing it announces**
//! (#251 AC3). [`service::notify`] takes `&mut PgTransaction` and returns its
//! error, exactly as [`crate::modules::activity::service::record`] does, and
//! for the same reason stated one module over: *an approval that rolled back
//! did not happen*, and telling somebody it did is worse than telling them
//! nothing. The other half is newer and is this module's own: **a notification
//! lost when the approval commits is the silence this feature exists to end**,
//! so it cannot be `record_or_warn`'s deliberate tolerance either.
//!
//! So the signature is the rule, and there is no way to reach
//! [`repository::insert`] with a pool.
//!
//! # One permission, and it is checked — which D-47 does not contradict
//!
//! `notification:read` gates the centre. **D-47 removed `activity:read` five
//! days after seeding it, so a new permission in this codebase owes an
//! explanation**, and the difference is what the permission answers.
//!
//! `activity:read` asked *may this account read timelines*, when the document's
//! own read had already answered the same question about the same rows — a
//! second lock on one door. `notification:read` asks *does this account have a
//! notification centre at all*, and **nothing else in the product answers it**:
//! a notification is addressed to a person rather than attached to a record
//! whose permission could stand in.
//!
//! **Row scoping is a separate rule and not this permission's job** (#251 AC7).
//! Holding `notification:read` lets an account read *its own* notifications; the
//! statement is what makes that true, and it would be true if the permission did
//! not exist. The two are independent, which is exactly what `activity:read`
//! was not.
//!
//! # What is not here
//!
//! **No email, no templates, no delivery log.** `0034_notification.sql` creates
//! those three tables and nothing writes them; FR-NTF-004 is
//! [#257](https://github.com/sujanto-gaws/kelir/issues/257). This release
//! composes its two sentences in [`service`], because one channel with two
//! message shapes does not need a template engine.
//!
//! **Nothing notifies about lateness** (#251 AC6). FR-NTF-006/007 are `Could`
//! and unscheduled, and they depend on FR-WF-010, which is also unscheduled —
//! [#185](https://github.com/sujanto-gaws/kelir/issues/185) made lateness
//! visible and nothing acts on it. A reminder in this release would be a
//! reminder nothing could ever stop sending.
//!
//! **No preferences.** FR-NTF-005 is unscheduled. Every account with the
//! permission gets every notification addressed to it, and the only control is
//! the permission.
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;
pub mod template;
pub mod worker;

/// Reading and dismissing **your own** notifications.
///
/// One permission rather than a read/update pair. Marking a notification read is
/// not a second capability: it is the act of having read the thing this
/// permission grants reading of, and an account that could see its notifications
/// and never clear them would be one this product had made worse rather than
/// safer. `master-data`'s read/update split exists because those are different
/// people; here they are the same person by definition of `recipient_user_id`.
pub const NOTIFICATION_READ: &str = "notification:read";
