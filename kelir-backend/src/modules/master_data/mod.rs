//! Party master data (SRS FR-MDM-001, FR-MDM-003).
//!
//! A party is a person or a party group; everything the business deals with —
//! supplier, customer, employee, contact — is a party holding a role, not a
//! table of its own (Database Schema §14 deviation #1). This module owns the
//! party and its attributes; the roles and their profiles are #81.
//!
//! Storage is Database Schema §4; the payload shape is the `PartyAggregate` of
//! architecture document 05. See `domain` for where the two disagree and why.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;
