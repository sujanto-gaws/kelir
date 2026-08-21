//! The roles a party holds, and the role-specific profiles that hang off them
//! (FR-MDM-002).
//!
//! One party, many roles — the Party model's whole point. A supplier is a party
//! holding the SUPPLIER role with a supplier profile, not a row in a
//! `mdm_suppliers` table (Database Schema §14 deviation #1), and the same party
//! can be a supplier and a customer at once without being stored twice.
//!
//! Three things about the mapping, each a decision rather than a transcription:
//!
//! * **`tenant` is missing from `profiles`.** The aggregate has a `tenantProfile`;
//!   §4.15 puts that data on `tenants.settings_json` and says there is no
//!   `mdm_tenant_profiles` table in v1. An empty member would claim a capability
//!   that does not exist.
//! * **`departmentPartyId` is `departmentId`, and it is a department.** The
//!   aggregate models a department as a party; §4.14 realizes it as the link to
//!   `departments`, which is a different table with a different key. Keeping the
//!   aggregate's name over a value that is not a party id would be the more
//!   confusing of the two options.
//! * **`tenantPartyId` is absent from every profile**, for the reason recorded
//!   as §14 deviation #16: it has no column, and `tenant_id` already answers it.
//!
//! Everything else follows the aggregate, including the awkward parts:
//! `customerSince` really is spelled without `Date` while the column is
//! `customer_since_date`, and money travels as a string because a JSON number is
//! an IEEE double and `NUMERIC(18,2)` has values it cannot hold exactly.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{bound_name, echoed_party_id, finish, non_empty, require_code, MAX_CODE_LENGTH};
use crate::error::{AppError, ValidationDetail};

/// The role type codes that have a profile table behind them (§4.12-4.15).
///
/// A tenant may add role types of its own without a migration (`mdm_role_types`
/// is an ordinary table); those carry no profile, which is why this is a lookup
/// rather than an enum over every role a party can hold.
pub const PROFILED_ROLE_TYPES: [&str; 4] = ["SUPPLIER", "CUSTOMER", "EMPLOYEE", "CONTACT"];

// ---------------------------------------------------------------------------
// Vocabularies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartyRoleStatus {
    Active,
    Inactive,
}

impl PartyRoleStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "INACTIVE" => Self::Inactive,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupplierApprovalStatus {
    Draft,
    Pending,
    Approved,
    Rejected,
    Blacklisted,
}

impl SupplierApprovalStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Pending => "PENDING",
            Self::Approved => "APPROVED",
            Self::Rejected => "REJECTED",
            Self::Blacklisted => "BLACKLISTED",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "PENDING" => Self::Pending,
            "APPROVED" => Self::Approved,
            "REJECTED" => Self::Rejected,
            "BLACKLISTED" => Self::Blacklisted,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EmploymentType {
    FullTime,
    PartTime,
    Contract,
    Intern,
    Outsourced,
}

impl EmploymentType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::FullTime => "FULL_TIME",
            Self::PartTime => "PART_TIME",
            Self::Contract => "CONTRACT",
            Self::Intern => "INTERN",
            Self::Outsourced => "OUTSOURCED",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "FULL_TIME" => Some(Self::FullTime),
            "PART_TIME" => Some(Self::PartTime),
            "CONTRACT" => Some(Self::Contract),
            "INTERN" => Some(Self::Intern),
            "OUTSOURCED" => Some(Self::Outsourced),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// A party's roles and the profiles behind them.
///
/// The two travel together because a profile without its role is meaningless —
/// a supplier profile on a party that is not a supplier describes nothing — and
/// they are gated by one permission, `master-data:party-role:read`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyRoles {
    pub roles: Vec<PartyRole>,
    pub profiles: PartyProfiles,
}

/// One role a party holds, over a period.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyRole {
    /// The role type's code — `SUPPLIER`, `EMPLOYEE`, or one a tenant added.
    pub role_type_id: String,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub status_id: PartyRoleStatus,
    pub comments: Option<String>,
    pub additional_attributes: Value,
}

/// The role-specific profiles a party carries. A member is present exactly when
/// the party holds the matching role and that role has a profile.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyProfiles {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supplier: Option<SupplierProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer: Option<CustomerProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub employee: Option<EmployeeProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<ContactProfile>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SupplierProfile {
    pub party_id: String,
    pub supplier_number: String,
    pub supplier_category: Option<String>,
    pub payment_term_days: Option<i32>,
    pub default_currency_uom: Option<String>,
    pub tax_number: Option<String>,
    pub bank_name: Option<String>,
    pub bank_account: Option<String>,
    pub bank_account_name: Option<String>,
    pub approval_status: SupplierApprovalStatus,
    pub status_id: Option<String>,
    pub additional_attributes: Value,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomerProfile {
    pub party_id: String,
    pub customer_number: String,
    pub customer_category: Option<String>,
    /// The aggregate spells this without `Date`; the column is
    /// `customer_since_date`.
    pub customer_since: Option<NaiveDate>,
    /// `NUMERIC(18,2)`, carried as a string so no precision is lost passing
    /// through a JSON double.
    pub credit_limit: Option<String>,
    pub payment_term_days: Option<i32>,
    pub default_currency_uom: Option<String>,
    pub tax_number: Option<String>,
    /// The `partyId` of the party billed for this customer, when it is not the
    /// customer itself.
    pub billing_party_id: Option<String>,
    pub status_id: Option<String>,
    pub additional_attributes: Value,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeProfile {
    pub party_id: String,
    pub employee_number: String,
    /// The aggregate's `departmentPartyId`, realized as the link to
    /// `departments` (§4.14) — so it is a department id, not a party id.
    pub department_id: Option<Uuid>,
    pub manager_party_id: Option<String>,
    pub position: Option<String>,
    pub job_grade: Option<String>,
    pub employment_type: Option<EmploymentType>,
    pub join_date: Option<NaiveDate>,
    pub resign_date: Option<NaiveDate>,
    pub status_id: Option<String>,
    pub additional_attributes: Value,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContactProfile {
    pub party_id: String,
    pub contact_type: Option<String>,
    pub preferred_contact_mech_type_id: Option<String>,
    pub do_not_contact: bool,
    pub assistant_party_id: Option<String>,
    pub additional_attributes: Value,
}

// ---------------------------------------------------------------------------
// Request shapes
// ---------------------------------------------------------------------------

/// Gives a party a role, or changes the one it already holds.
///
/// The role type is in the path, so it is not repeated here. Every field is the
/// state the assignment should end in — `PUT` is idempotent, and assigning a
/// role a party already holds is an update rather than a second row.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignRoleRequest {
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub status_id: Option<PartyRoleStatus>,
    pub comments: Option<String>,
    pub additional_attributes: Option<Value>,
    /// The profile for this role. Required for the four role types that have
    /// one, refused for the ones that do not.
    pub profile: Option<RoleProfileInput>,
}

/// The profile of whichever role is being assigned. Exactly one member, and it
/// has to be the one matching the role type in the path.
///
/// It is shaped as the aggregate's `profiles` object rather than as an untagged
/// union so that the member names are the same on the way in and the way out,
/// and so a client that sends the wrong one is told which it sent.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleProfileInput {
    pub supplier: Option<SupplierProfileInput>,
    pub customer: Option<CustomerProfileInput>,
    pub employee: Option<EmployeeProfileInput>,
    pub contact: Option<ContactProfileInput>,
}

impl RoleProfileInput {
    /// The role type code this profile belongs to, or `None` when no member is
    /// set. Returns the first match; [`validate_assign_role`] refuses more than
    /// one before this is consulted.
    pub fn role_type(&self) -> Option<&'static str> {
        if self.supplier.is_some() {
            Some("SUPPLIER")
        } else if self.customer.is_some() {
            Some("CUSTOMER")
        } else if self.employee.is_some() {
            Some("EMPLOYEE")
        } else if self.contact.is_some() {
            Some("CONTACT")
        } else {
            None
        }
    }

    fn members_set(&self) -> usize {
        usize::from(self.supplier.is_some())
            + usize::from(self.customer.is_some())
            + usize::from(self.employee.is_some())
            + usize::from(self.contact.is_some())
    }
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupplierProfileInput {
    pub party_id: Option<String>,
    pub supplier_number: Option<String>,
    pub supplier_category: Option<String>,
    pub payment_term_days: Option<i32>,
    pub default_currency_uom: Option<String>,
    pub tax_number: Option<String>,
    pub bank_name: Option<String>,
    pub bank_account: Option<String>,
    pub bank_account_name: Option<String>,
    pub approval_status: Option<SupplierApprovalStatus>,
    pub status_id: Option<String>,
    pub additional_attributes: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomerProfileInput {
    pub party_id: Option<String>,
    pub customer_number: Option<String>,
    pub customer_category: Option<String>,
    pub customer_since: Option<NaiveDate>,
    /// Decimal as a string, matching the response.
    pub credit_limit: Option<String>,
    pub payment_term_days: Option<i32>,
    pub default_currency_uom: Option<String>,
    pub tax_number: Option<String>,
    pub billing_party_id: Option<String>,
    pub status_id: Option<String>,
    pub additional_attributes: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmployeeProfileInput {
    pub party_id: Option<String>,
    pub employee_number: Option<String>,
    pub department_id: Option<Uuid>,
    pub manager_party_id: Option<String>,
    pub position: Option<String>,
    pub job_grade: Option<String>,
    pub employment_type: Option<EmploymentType>,
    pub join_date: Option<NaiveDate>,
    pub resign_date: Option<NaiveDate>,
    pub status_id: Option<String>,
    pub additional_attributes: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactProfileInput {
    pub party_id: Option<String>,
    pub contact_type: Option<String>,
    pub preferred_contact_mech_type_id: Option<String>,
    pub do_not_contact: Option<bool>,
    pub assistant_party_id: Option<String>,
    pub additional_attributes: Option<Value>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validates a role assignment against the role type it is being made under.
///
/// `creating` distinguishes the first assignment, where the profile's business
/// number is required, from a change to one that already exists, where an
/// unsent field means *leave alone*.
pub fn validate_assign_role(
    request: &AssignRoleRequest,
    role_type_code: &str,
    party_code: &str,
    creating: bool,
) -> Result<(), AppError> {
    let mut details = Vec::new();
    let role_type = role_type_code.trim();

    require_code(role_type, "roleTypeId", MAX_CODE_LENGTH, &mut details);

    if let Some(thru) = request.thru_date {
        if thru < request.from_date {
            details.push(ValidationDetail::new(
                "thruDate",
                "range",
                "OUT_OF_RANGE",
                "thruDate cannot be before fromDate",
            ));
        }
    }

    let expects_profile = PROFILED_ROLE_TYPES.contains(&role_type);

    match &request.profile {
        None if expects_profile && creating => details.push(ValidationDetail::new(
            "profile",
            "required",
            "REQUIRED",
            format!(
                "The {role_type} role carries a profile; send one under profile.{}",
                role_type.to_lowercase()
            ),
        )),
        None => {}
        Some(profile) => {
            if !expects_profile {
                details.push(ValidationDetail::new(
                    "profile",
                    "conflict",
                    "NOT_ALLOWED",
                    format!("The {role_type} role has no profile"),
                ));
            } else if profile.members_set() > 1 {
                details.push(ValidationDetail::new(
                    "profile",
                    "conflict",
                    "NOT_ALLOWED",
                    "Send one profile — the one belonging to the role being assigned",
                ));
            } else {
                match profile.role_type() {
                    None => details.push(ValidationDetail::new(
                        "profile",
                        "required",
                        "REQUIRED",
                        "profile carries no member",
                    )),
                    Some(sent) if sent != role_type => details.push(ValidationDetail::new(
                        format!("profile.{}", sent.to_lowercase()),
                        "consistency",
                        "MISMATCH",
                        format!(
                            "This is the {sent} profile; the role being assigned is {role_type}"
                        ),
                    )),
                    Some(_) => {
                        validate_profile(profile, party_code, creating, &mut details);
                    }
                }
            }
        }
    }

    finish(details)
}

fn validate_profile(
    profile: &RoleProfileInput,
    party_code: &str,
    creating: bool,
    details: &mut Vec<ValidationDetail>,
) {
    if let Some(supplier) = &profile.supplier {
        let path = "profile.supplier";
        echoed_party_id(supplier.party_id.as_deref(), party_code, path, details);
        require_business_number(
            supplier.supplier_number.as_deref(),
            &format!("{path}.supplierNumber"),
            creating,
            details,
        );
        bound_name(
            supplier.bank_name.as_deref(),
            &format!("{path}.bankName"),
            details,
        );
        bound_name(
            supplier.bank_account_name.as_deref(),
            &format!("{path}.bankAccountName"),
            details,
        );
        bound_code(
            supplier.supplier_category.as_deref(),
            &format!("{path}.supplierCategory"),
            details,
        );
        bound_code(
            supplier.tax_number.as_deref(),
            &format!("{path}.taxNumber"),
            details,
        );
        bound_code(
            supplier.bank_account.as_deref(),
            &format!("{path}.bankAccount"),
            details,
        );
    }

    if let Some(customer) = &profile.customer {
        let path = "profile.customer";
        echoed_party_id(customer.party_id.as_deref(), party_code, path, details);
        require_business_number(
            customer.customer_number.as_deref(),
            &format!("{path}.customerNumber"),
            creating,
            details,
        );
        bound_code(
            customer.customer_category.as_deref(),
            &format!("{path}.customerCategory"),
            details,
        );
        bound_code(
            customer.tax_number.as_deref(),
            &format!("{path}.taxNumber"),
            details,
        );
        require_decimal(
            customer.credit_limit.as_deref(),
            &format!("{path}.creditLimit"),
            details,
        );
    }

    if let Some(employee) = &profile.employee {
        let path = "profile.employee";
        echoed_party_id(employee.party_id.as_deref(), party_code, path, details);
        require_business_number(
            employee.employee_number.as_deref(),
            &format!("{path}.employeeNumber"),
            creating,
            details,
        );
        bound_name(
            employee.position.as_deref(),
            &format!("{path}.position"),
            details,
        );
        bound_code(
            employee.job_grade.as_deref(),
            &format!("{path}.jobGrade"),
            details,
        );

        if let (Some(join), Some(resign)) = (employee.join_date, employee.resign_date) {
            if resign < join {
                details.push(ValidationDetail::new(
                    format!("{path}.resignDate"),
                    "range",
                    "OUT_OF_RANGE",
                    "resignDate cannot be before joinDate",
                ));
            }
        }
    }

    if let Some(contact) = &profile.contact {
        let path = "profile.contact";
        echoed_party_id(contact.party_id.as_deref(), party_code, path, details);
        bound_code(
            contact.contact_type.as_deref(),
            &format!("{path}.contactType"),
            details,
        );
    }
}

/// A profile's business number: required when the profile is being created,
/// and never blank when it is sent.
fn require_business_number(
    value: Option<&str>,
    path: &str,
    creating: bool,
    details: &mut Vec<ValidationDetail>,
) {
    match non_empty(value) {
        Some(number) => require_code(number, path, MAX_CODE_LENGTH, details),
        None if creating || value.is_some() => details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "This field is required",
        )),
        None => {}
    }
}

fn bound_code(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    if let Some(code) = non_empty(value) {
        if code.chars().count() > MAX_CODE_LENGTH {
            details.push(ValidationDetail::new(
                path,
                "maxLength",
                "TOO_LONG",
                format!("Must be at most {MAX_CODE_LENGTH} characters"),
            ));
        }
    }
}

fn require_decimal(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    if let Some(amount) = non_empty(value) {
        if amount.parse::<f64>().is_err() {
            details.push(ValidationDetail::new(
                path,
                "format",
                "INVALID_FORMAT",
                "Must be a decimal number",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(0, 0).expect("epoch is a valid timestamp")
    }

    fn supplier_request() -> AssignRoleRequest {
        AssignRoleRequest {
            from_date: epoch(),
            thru_date: None,
            status_id: None,
            comments: None,
            additional_attributes: None,
            profile: Some(RoleProfileInput {
                supplier: Some(SupplierProfileInput {
                    supplier_number: Some("SUP-0001".to_owned()),
                    ..SupplierProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
        }
    }

    fn paths(error: AppError) -> Vec<String> {
        match error {
            AppError::Validation { details } => {
                details.into_iter().map(|detail| detail.path).collect()
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_supplier_assignment() {
        assert!(validate_assign_role(&supplier_request(), "SUPPLIER", "PARTY-0001", true).is_ok());
    }

    #[test]
    fn a_profiled_role_needs_its_profile_when_it_is_first_assigned() {
        // Without this, a party could hold the SUPPLIER role with no supplier
        // number — a supplier nothing can raise a purchase order against.
        let bare = AssignRoleRequest {
            profile: None,
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&bare, "SUPPLIER", "PARTY-0001", true).expect_err("invalid")
        )
        .contains(&"profile".to_owned()));

        // On a change to an assignment that already exists, an absent profile
        // means "leave it alone".
        assert!(validate_assign_role(&bare, "SUPPLIER", "PARTY-0001", false).is_ok());
    }

    #[test]
    fn the_profile_must_be_the_one_the_role_belongs_to() {
        // A customer profile sent while assigning SUPPLIER would otherwise be
        // written to the supplier table or silently dropped, depending on how
        // the service read it.
        let mismatched = AssignRoleRequest {
            profile: Some(RoleProfileInput {
                customer: Some(CustomerProfileInput {
                    customer_number: Some("CUS-0001".to_owned()),
                    ..CustomerProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&mismatched, "SUPPLIER", "PARTY-0001", true).expect_err("invalid")
        )
        .contains(&"profile.customer".to_owned()));
    }

    #[test]
    fn only_one_profile_may_be_sent() {
        let both = AssignRoleRequest {
            profile: Some(RoleProfileInput {
                supplier: Some(SupplierProfileInput {
                    supplier_number: Some("SUP-0001".to_owned()),
                    ..SupplierProfileInput::default()
                }),
                customer: Some(CustomerProfileInput {
                    customer_number: Some("CUS-0001".to_owned()),
                    ..CustomerProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&both, "SUPPLIER", "PARTY-0001", true).expect_err("invalid")
        )
        .contains(&"profile".to_owned()));
    }

    #[test]
    fn a_role_type_with_no_profile_refuses_one() {
        // ORGANIZATION_UNIT is seeded and has no profile table. Accepting a
        // profile for it would accept data with nowhere to go.
        let request = supplier_request();

        assert!(paths(
            validate_assign_role(&request, "ORGANIZATION_UNIT", "PARTY-0001", true)
                .expect_err("invalid")
        )
        .contains(&"profile".to_owned()));
    }

    #[test]
    fn a_tenant_defined_role_type_assigns_without_a_profile() {
        // Acceptance criterion 4: a tenant adds a role type without a
        // migration, and it has to be assignable.
        let bare = AssignRoleRequest {
            profile: None,
            ..supplier_request()
        };

        assert!(validate_assign_role(&bare, "AUDITOR", "PARTY-0001", true).is_ok());
    }

    #[test]
    fn the_profile_number_is_required_on_creation_and_never_blank() {
        let blank = AssignRoleRequest {
            profile: Some(RoleProfileInput {
                supplier: Some(SupplierProfileInput {
                    supplier_number: Some("   ".to_owned()),
                    ..SupplierProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
            ..supplier_request()
        };

        for creating in [true, false] {
            assert!(
                paths(
                    validate_assign_role(&blank, "SUPPLIER", "PARTY-0001", creating)
                        .expect_err("invalid")
                )
                .contains(&"profile.supplier.supplierNumber".to_owned()),
                "a blank number must be refused whether creating ({creating}) or not"
            );
        }
    }

    #[test]
    fn an_echoed_party_id_must_agree_with_the_party() {
        let disagreeing = AssignRoleRequest {
            profile: Some(RoleProfileInput {
                supplier: Some(SupplierProfileInput {
                    party_id: Some("SOMEONE-ELSE".to_owned()),
                    supplier_number: Some("SUP-0001".to_owned()),
                    ..SupplierProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&disagreeing, "SUPPLIER", "PARTY-0001", true)
                .expect_err("invalid")
        )
        .contains(&"profile.supplier.partyId".to_owned()));
    }

    #[test]
    fn a_role_cannot_end_before_it_starts() {
        let backwards = AssignRoleRequest {
            thru_date: Some(epoch() - chrono::Duration::days(1)),
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&backwards, "SUPPLIER", "PARTY-0001", true).expect_err("invalid")
        )
        .contains(&"thruDate".to_owned()));
    }

    #[test]
    fn an_employee_cannot_resign_before_joining() {
        let backwards = AssignRoleRequest {
            profile: Some(RoleProfileInput {
                employee: Some(EmployeeProfileInput {
                    employee_number: Some("EMP-0001".to_owned()),
                    join_date: NaiveDate::from_ymd_opt(2026, 3, 1),
                    resign_date: NaiveDate::from_ymd_opt(2026, 1, 1),
                    ..EmployeeProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&backwards, "EMPLOYEE", "PARTY-0001", true).expect_err("invalid")
        )
        .contains(&"profile.employee.resignDate".to_owned()));
    }

    #[test]
    fn a_credit_limit_that_is_not_a_number_is_refused() {
        // It reaches a NUMERIC column through a cast; a bad value would be a
        // 500 from PostgreSQL rather than a message naming the field.
        let bad = AssignRoleRequest {
            profile: Some(RoleProfileInput {
                customer: Some(CustomerProfileInput {
                    customer_number: Some("CUS-0001".to_owned()),
                    credit_limit: Some("a lot".to_owned()),
                    ..CustomerProfileInput::default()
                }),
                ..RoleProfileInput::default()
            }),
            ..supplier_request()
        };

        assert!(paths(
            validate_assign_role(&bad, "CUSTOMER", "PARTY-0001", true).expect_err("invalid")
        )
        .contains(&"profile.customer.creditLimit".to_owned()));
    }

    #[test]
    fn vocabularies_round_trip_through_the_database() {
        for status in [PartyRoleStatus::Active, PartyRoleStatus::Inactive] {
            assert_eq!(PartyRoleStatus::from_db(status.as_db()), status);
        }
        for status in [
            SupplierApprovalStatus::Draft,
            SupplierApprovalStatus::Pending,
            SupplierApprovalStatus::Approved,
            SupplierApprovalStatus::Rejected,
            SupplierApprovalStatus::Blacklisted,
        ] {
            assert_eq!(SupplierApprovalStatus::from_db(status.as_db()), status);
        }
        for employment in [
            EmploymentType::FullTime,
            EmploymentType::PartTime,
            EmploymentType::Contract,
            EmploymentType::Intern,
            EmploymentType::Outsourced,
        ] {
            assert_eq!(
                EmploymentType::from_db(employment.as_db()),
                Some(employment)
            );
        }
    }

    #[test]
    fn every_profiled_role_type_is_one_the_migration_seeds() {
        // `0008_master_data.sql` seeds six role types; the four with profile
        // tables have to be among them, or a profile could never be written.
        for role_type in PROFILED_ROLE_TYPES {
            assert!(
                [
                    "TENANT",
                    "EMPLOYEE",
                    "CUSTOMER",
                    "SUPPLIER",
                    "CONTACT",
                    "ORGANIZATION_UNIT"
                ]
                .contains(&role_type),
                "{role_type} has a profile table but is not a seeded role type"
            );
        }
    }
}
