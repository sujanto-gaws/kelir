//! Menu definitions — the navigation a deployment configures (FR-RAD-004,
//! [#341]).
//!
//! **`rad_menus` has been in the schema since `0014_rad.sql` and had no
//! reader.** [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
//! §5.13 said why that was right at the time — *a permission row that no route
//! checks reads as a control that exists* — and it is why this file arrives
//! with a route and a migration rather than ahead of them.
//!
//! # A configured menu joins the built-in navigation rather than replacing it
//!
//! The frontend's eight destinations are the `CORE` source and stay in code;
//! rows here are `CONFIG`, and the sidebar merges the two by `sortOrder` with
//! every entry still hidden by its own `requiredPermission`. **A tenant that
//! configures nothing sees exactly the navigation it saw yesterday**, which is
//! the property that makes this additive: the alternative — seeding the
//! built-ins as rows and rendering only the table — makes every tenant's
//! navigation depend on a seed being right, and an empty table an application
//! nobody can navigate.
//!
//! # `requiredPermission` hides; it does not guard
//!
//! §5.9's own column comment is *hide when the user lacks it*, and that is the
//! whole of what it does. A menu entry is a link, and the screen it points at
//! enforces its own permission — `router/index.ts`'s `meta.permission` and the
//! endpoint behind it. **Hiding is cosmetic and is not a control**, which the
//! frontend's own navigation comment already says; a menu row naming a
//! permission nobody holds removes a link and opens nothing.
//!
//! [#341]: https://github.com/sujanto-gaws/kelir/issues/341

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::utils::serde::present_or_absent;

/// Longest `menu_key` §5.9 holds — `menu_key VARCHAR(64)`.
pub const MAX_MENU_KEY_LENGTH: usize = 64;
/// Longest `label` §5.9 holds — `VARCHAR(200)`.
pub const MAX_LABEL_LENGTH: usize = 200;
/// Longest `icon` and `required_permission` hold — `VARCHAR(64)`.
pub const MAX_SHORT_LENGTH: usize = 64;
/// Longest `route_path` holds — `VARCHAR(2048)`.
pub const MAX_ROUTE_LENGTH: usize = 2048;

/// How deep a menu tree may be walked when checking for a cycle.
///
/// **A safety margin, not a product limit.** Eight levels of navigation is
/// already unusable; the bound exists so a walk over a tree that is *already*
/// cyclic terminates. `facility_ancestors` carries the same argument at length,
/// including the reason a truncated walk must refuse rather than assume.
pub const MAX_MENU_DEPTH: i32 = 32;

/// Where a menu entry came from (§5.9's `CHECK`).
///
/// **`Config` is the only one this API writes.** `Core` is the navigation
/// compiled into the frontend and `Plugin` belongs to a runtime that does not
/// exist; both are in the column because the schema's merge is across all
/// three, and accepting either through the builder would let a deployment claim
/// a provenance the row does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MenuSource {
    Core,
    Config,
    Plugin,
}

impl MenuSource {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Core => "CORE",
            Self::Config => "CONFIG",
            Self::Plugin => "PLUGIN",
        }
    }

    /// An unknown stored value reads as `Plugin`, which is the variant this
    /// build renders and never writes — so a row from a future release appears
    /// in the navigation rather than vanishing from it, and cannot be mistaken
    /// for something this API produced.
    pub fn from_db(value: &str) -> Self {
        match value {
            "CORE" => Self::Core,
            "CONFIG" => Self::Config,
            _ => Self::Plugin,
        }
    }
}

/// A menu entry as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuEntry {
    pub id: Uuid,
    pub menu_key: String,
    pub label: String,
    /// A Lucide icon name, which the frontend resolves against the set it
    /// bundles. An unknown name renders without an icon rather than refusing —
    /// the icon set is the client's and a definition outlives a release of it.
    pub icon: Option<String>,
    pub parent_menu_id: Option<Uuid>,
    pub route_path: Option<String>,
    pub required_permission: Option<String>,
    pub source: MenuSource,
    pub sort_order: i32,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMenuRequest {
    pub menu_key: String,
    pub label: String,
    pub icon: Option<String>,
    pub parent_menu_id: Option<Uuid>,
    pub route_path: Option<String>,
    pub required_permission: Option<String>,
    pub sort_order: Option<i32>,
    pub is_enabled: Option<bool>,
}

/// Editing a menu entry. `None` means *leave alone*, the convention every other
/// update request in this module uses.
///
/// `menuKey` is absent because it may not change: it is the tenant-unique name
/// §5.9's index enforces and what a deployment's own documentation calls an
/// entry by.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMenuRequest {
    pub label: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub icon: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub parent_menu_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub route_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub required_permission: Option<Option<String>>,
    pub sort_order: Option<i32>,
    pub is_enabled: Option<bool>,
}

fn bounded(
    value: &str,
    path: &str,
    max: usize,
    required: bool,
    details: &mut Vec<ValidationDetail>,
) {
    let trimmed = value.trim();

    if required && trimmed.is_empty() {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            format!("{path} is required"),
        ));
    } else if trimmed.chars().count() > max {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("{path} must be at most {max} characters"),
        ));
    }
}

/// A route path a menu entry may point at.
///
/// **Relative to the application, and refused otherwise.** A menu row is
/// rendered as an in-application link; a value like `https://example.test` or
/// `javascript:…` would either navigate somewhere the router cannot go or, on a
/// client that used it directly, become a stored cross-site scripting vector.
/// The column is `VARCHAR(2048)` and the schema's own examples are `/documents`
/// and `/master-data/parties`.
///
/// An entry with **no** route is a heading — the parent of other entries — which
/// is why the field is optional rather than required.
fn check_route(route: Option<&str>, details: &mut Vec<ValidationDetail>) {
    let Some(route) = route.map(str::trim).filter(|route| !route.is_empty()) else {
        return;
    };

    bounded(route, "routePath", MAX_ROUTE_LENGTH, false, details);

    if !route.starts_with('/') || route.starts_with("//") {
        details.push(ValidationDetail::new(
            "routePath",
            "format",
            "ROUTE_NOT_RELATIVE",
            "a menu entry links somewhere in this application, so its route starts with a \
             single `/` — an absolute URL is not something the navigation can open",
        ));
    }
}

pub fn validate_create_menu(request: &CreateMenuRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    bounded(
        &request.menu_key,
        "menuKey",
        MAX_MENU_KEY_LENGTH,
        true,
        &mut details,
    );
    bounded(
        &request.label,
        "label",
        MAX_LABEL_LENGTH,
        true,
        &mut details,
    );

    if let Some(icon) = &request.icon {
        bounded(icon, "icon", MAX_SHORT_LENGTH, false, &mut details);
    }

    if let Some(permission) = &request.required_permission {
        bounded(
            permission,
            "requiredPermission",
            MAX_SHORT_LENGTH,
            false,
            &mut details,
        );
    }

    check_route(request.route_path.as_deref(), &mut details);
    finish(details)
}

pub fn validate_update_menu(request: &UpdateMenuRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(label) = &request.label {
        bounded(label, "label", MAX_LABEL_LENGTH, true, &mut details);
    }

    if let Some(Some(icon)) = &request.icon {
        bounded(icon, "icon", MAX_SHORT_LENGTH, false, &mut details);
    }

    if let Some(Some(permission)) = &request.required_permission {
        bounded(
            permission,
            "requiredPermission",
            MAX_SHORT_LENGTH,
            false,
            &mut details,
        );
    }

    if let Some(route) = &request.route_path {
        check_route(route.as_deref(), &mut details);
    }

    finish(details)
}

fn finish(details: Vec<ValidationDetail>) -> Result<(), AppError> {
    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create() -> CreateMenuRequest {
        CreateMenuRequest {
            menu_key: "purchasing".to_owned(),
            label: "Purchasing".to_owned(),
            icon: Some("shopping-cart".to_owned()),
            parent_menu_id: None,
            route_path: Some("/lists/purchase_requisitions".to_owned()),
            required_permission: Some("document:read".to_owned()),
            sort_order: Some(10),
            is_enabled: Some(true),
        }
    }

    fn details(error: AppError) -> Vec<ValidationDetail> {
        match error {
            AppError::Validation { details } => details,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_complete_entry() {
        assert!(validate_create_menu(&create()).is_ok());
    }

    #[test]
    fn requires_a_key_and_a_label() {
        let mut request = create();

        request.menu_key = "  ".to_owned();
        request.label = String::new();

        let details = details(validate_create_menu(&request).expect_err("refused"));

        assert!(details.iter().any(|detail| detail.path == "menuKey"));
        assert!(details.iter().any(|detail| detail.path == "label"));
    }

    #[test]
    fn refuses_a_key_longer_than_the_column() {
        let mut request = create();

        request.menu_key = "k".repeat(MAX_MENU_KEY_LENGTH + 1);

        assert!(
            details(validate_create_menu(&request).expect_err("refused"))
                .iter()
                .any(|detail| detail.path == "menuKey" && detail.code == "TOO_LONG")
        );
    }

    // -- The route -------------------------------------------------------

    /// **A menu entry links inside this application.** An absolute URL either
    /// goes somewhere the router cannot, or — on a client that used it directly
    /// — becomes a stored script the navigation renders for everybody.
    #[test]
    fn refuses_a_route_that_leaves_the_application() {
        for route in [
            "https://example.test/steal",
            "javascript:alert(1)",
            "//example.test/protocol-relative",
            "documents",
        ] {
            let mut request = create();

            request.route_path = Some(route.to_owned());

            let details = details(validate_create_menu(&request).expect_err("refused"));

            assert!(
                details
                    .iter()
                    .any(|detail| detail.code == "ROUTE_NOT_RELATIVE"),
                "`{route}` was accepted"
            );
        }
    }

    #[test]
    fn accepts_the_routes_the_schema_gives_as_examples() {
        for route in ["/documents", "/master-data/parties"] {
            let mut request = create();

            request.route_path = Some(route.to_owned());

            assert!(validate_create_menu(&request).is_ok(), "`{route}` refused");
        }
    }

    /// An entry with no route is a heading, which is what a parent is.
    #[test]
    fn a_menu_with_no_route_is_a_heading_rather_than_a_mistake() {
        let mut request = create();

        request.route_path = None;

        assert!(validate_create_menu(&request).is_ok());

        request.route_path = Some("   ".to_owned());

        assert!(validate_create_menu(&request).is_ok());
    }

    // -- The update ------------------------------------------------------

    #[test]
    fn an_update_that_changes_nothing_is_valid() {
        let request = UpdateMenuRequest {
            label: None,
            icon: None,
            parent_menu_id: None,
            route_path: None,
            required_permission: None,
            sort_order: None,
            is_enabled: None,
        };

        assert!(validate_update_menu(&request).is_ok());
    }

    #[test]
    fn an_update_checks_the_route_it_carries() {
        let request = UpdateMenuRequest {
            label: None,
            icon: None,
            parent_menu_id: None,
            route_path: Some(Some("https://example.test".to_owned())),
            required_permission: None,
            sort_order: None,
            is_enabled: None,
        };

        assert!(validate_update_menu(&request).is_err());
    }

    /// Clearing a route is how an entry becomes a heading, and must not be read
    /// as an empty route to validate.
    #[test]
    fn an_update_may_clear_a_route() {
        let request = UpdateMenuRequest {
            label: None,
            icon: None,
            parent_menu_id: None,
            route_path: Some(None),
            required_permission: None,
            sort_order: None,
            is_enabled: None,
        };

        assert!(validate_update_menu(&request).is_ok());
    }

    // -- The source ------------------------------------------------------

    #[test]
    fn reads_every_source_the_check_allows() {
        assert_eq!(MenuSource::from_db("CORE"), MenuSource::Core);
        assert_eq!(MenuSource::from_db("CONFIG"), MenuSource::Config);
        assert_eq!(MenuSource::from_db("PLUGIN"), MenuSource::Plugin);
    }

    /// A row from a future release appears in the navigation rather than
    /// vanishing from it, and cannot be mistaken for one this API wrote.
    #[test]
    fn an_unknown_source_reads_as_plugin_rather_than_config() {
        assert_eq!(MenuSource::from_db("EXTENSION"), MenuSource::Plugin);
    }
}
