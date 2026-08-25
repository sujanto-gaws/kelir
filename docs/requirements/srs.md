# Kelir Software Requirements Specification

**Status:** Draft · **Last updated:** 2026-08-25

The companion Solution Blueprint formerly bundled in this file now lives in the System Design Document: `docs/design/01. System Design Document.md`.

---

# Document 1: Kelir Software Requirements Specification

## Document Control

| Item | Detail |
|---|---|
| Document Name | Kelir Software Requirements Specification |
| Framework Name | Kelir |
| Version | 0.8 |
| Status | Initial Draft |
| Date | 2026-08-05 |
| Document Type | SRS |
| Architecture Style | Full-stack document-based workflow platform |
| Backend | Rust |
| Frontend | Vue + Vite + Axios + shadcn-vue + Tailwind CSS v4 |
| Database | PostgreSQL, optional MariaDB compatibility |

Revision history:

```text
0.1 (2026-08-05): initial draft.
0.2 (2026-08-05): consistency refinements from documentation audit.
0.3 (2026-08-05): adopted the unified Party model for supplier, customer, and employee master data (see docs/architectures/05. Core - Master Data - Party.md).
0.4 (2026-08-06): split the Solution Blueprint out into the System Design Document (docs/design/01. System Design Document.md); this file now contains the SRS only.
0.5 (2026-08-11): separated priority from MVP scope — Must now means "required for 1.0" and §9 is the sole MVP gate (§4 preamble); raised FR-API-004 (OpenAPI) from Should to Must, since §9 criterion 14 requires documented APIs; baselined the six proposed targets in FR-ATT-004, NFR-PERF-001, NFR-AVA-004 and NFR-SEC-008/009/010. Recorded as decisions D-3 and D-5 in projects/planning/02. Product Backlog.md.
0.6 (2026-08-20): narrowed FR-IDM-004 from "manage permissions" to maintaining the permission catalogue, which is system-defined rather than administrator-editable; the administrative surface is role–permission mapping (FR-IDM-005). No requirement added or removed, and MVP scope is unchanged — §9 names neither. Recorded as decision D-6 in projects/planning/02. Product Backlog.md.
0.7 (2026-08-20): re-scoped FR-IDM-008 to department assignment, leaving department management to FR-ORG-002 and positions to FR-ORG-003, which the three requirements had been claiming between them; recorded that multi-tenant mode (FR-IDM-009) is not exercised before 1.0 and that a deployment serves one tenant, added to §10. No requirement added or removed, and MVP scope is unchanged — §9 names neither departments nor tenants. Recorded as decisions D-7 and D-8 in projects/planning/02. Product Backlog.md.
0.8 (2026-08-25): reversed the v0.7 tenancy entry. Multi-tenant mode is exercised: FR-IDM-009 is delivered in full and FR-ORG-001 with it, so the §10 line deferring multiple tenants past 1.0 is removed and the §4.2 and §4.3 notes are rewritten to say what was built rather than what was deferred. §2 gains the answer FR-IDM-009 had left open — roles are tenant-scoped, the permission catalogue is global. No requirement added or removed, and MVP scope is unchanged — §9 still names no tenant criterion, which is why this could be `Should` work at all. Recorded as decision D-18 in projects/planning/02. Product Backlog.md, superseding D-7.
```

> **Note:** As of v0.4 this file contains only the SRS. The Solution Blueprint has been split out into the System Design Document (`docs/design/01. System Design Document.md`), which is versioned independently.

---

# 1. Introduction

## 1.1 Purpose

The purpose of this document is to define the initial functional and non-functional requirements for **Kelir**, a document-based business application framework with workflow-driven processing and rapid application development capabilities.

Kelir is intended to provide a foundation for building business applications such as:

- Document approval systems
- Procurement workflows
- Invoice approval
- Vendor registration
- Employee onboarding
- Contract management
- Asset requests
- Facility management
- Master data governance
- Compliance and audit-ready document processing

---

## 1.2 Scope

Kelir is a full-stack framework that provides:

```text
Document management
Workflow engine
Task inbox and approvals
Master data management
Attachment management
Comments and collaboration
Activity log
Audit trail
Notifications
Reporting
Rapid application development metadata
Integration layer
Plugin/extension management
Authentication and authorization
API-first architecture
```

The initial version will focus on a modular monolith architecture with REST APIs and a Vue-based frontend.

---

## 1.3 Intended Audience

This document is intended for:

```text
Project sponsors
Product owners
Solution architects
Backend developers
Frontend developers
QA engineers
DevOps engineers
Security reviewers
Business analysts
Implementation teams
```

---

## 1.4 Definitions and Acronyms

| Term | Meaning |
|---|---|
| Kelir | The framework name |
| Document | A business transaction object such as request, form, approval, contract, invoice |
| Document Type | Definition of a class of documents, form, numbering, workflow, rules |
| Workflow | Controlled process flow with states, tasks, transitions, and approvals |
| Task | Work item assigned to user or role |
| Master Data | Core reference data: parties (suppliers, customers, employees) plus facilities, products, and services |
| Party | Unified master-data entity representing a person or an organization (party group); suppliers, customers, and employees are parties holding the corresponding role with a role-specific profile (see docs/architectures/05. Core - Master Data - Party.md) |
| RAD | Rapid Application Development |
| Plugin | Extension that adds features to Kelir |
| Integration | Connection with external systems |
| Audit Trail | Immutable record of business and system actions |
| RBAC | Role-Based Access Control |
| ABAC | Attribute-Based Access Control |
| MDM | Master Data Management |
| Multi-tenant mode | A deployment configuration flag; when enabled, all business data is partitioned by tenant_id and tenant administrators manage users within their tenant. Single-tenant deployments run with a single default tenant, and a caller naming a tenant is ignored rather than obeyed. Roles are tenant-scoped — every tenant has its own copy of a system role — while the permission catalogue is global (decision D-18) |
| JWT | JSON Web Token, a signed token format used for stateless authentication |
| SSO | Single Sign-On, authenticating once to access multiple applications |
| OAuth2 | An authorization framework for delegated access to resources |
| OIDC | OpenID Connect, an identity layer on top of OAuth2 |
| MVP | Minimum Viable Product, the smallest feature set acceptable for first release |
| CRUD | Create, Read, Update, Delete operations |
| JSONB | PostgreSQL binary JSON column type supporting indexing and querying |
| MoSCoW | Prioritization scheme: Must, Should, Could, Won't |
| MinIO | S3-compatible object storage server used for attachments |
| Mailpit | Local SMTP server with web UI used for email testing in development |
| Outbox pattern | Persisting outbound events in a database table within the same transaction as the business change, for reliable asynchronous delivery |

---

# 2. Overall Description

## 2.1 Product Perspective

Kelir is a document-centric business application framework.

The core concept is:

```text
Every business transaction is represented as a document.
Every document moves through a controlled workflow.
Every action is recorded in activity and audit logs.
Master data is governed separately but can be updated through controlled document workflows.
Additional features can be added through configuration, extensions, and plugins.
```

---

## 2.2 User Classes

| User Class | Description |
|---|---|
| Employee / Requester | Creates and submits documents |
| Approver | Reviews, approves, rejects, or returns documents |
| Department Manager | Approves documents within department scope |
| Finance / Legal / Compliance Officer | Reviews specialized documents |
| Administrator | Manages users, roles, document types, workflows, and configuration |
| Tenant Administrator | Manages configuration for a tenant. Holds every permission within it and none over the set of tenants — creating and suspending tenants is done from the deployment's default tenant only (FR-ORG-001, decision D-18) |
| Auditor | Views audit trail, activity logs, and compliance reports |
| System Integrator | Manages integration with external systems |
| Plugin Developer | Develops extensions or plugins |
| Developer | Extends Kelir core or custom modules |

---

## 2.3 Operating Environment

Initial target environment:

```text
Backend runtime: Linux container
Frontend hosting: Static files served by Nginx or CDN
Database: PostgreSQL 15+ or compatible
Object storage: Local storage or S3-compatible storage
Browser: Modern web browser
Authentication: JWT-based or session-based
Deployment: Docker Compose or Kubernetes
```

---

## 2.4 Assumptions

The following assumptions apply to the initial version:

```text
Kelir will start as a modular monolith.
PostgreSQL will be the primary database.
MariaDB support is optional and may require compatibility limitations.
Workflow engine will be a lightweight internal engine first.
Full BPMN integration may be added later.
Plugins will initially be configuration-based or compiled-in modules.
Third-party dynamic plugins will be introduced in later phases.
```

---

## 2.5 Constraints

```text
Backend must use Rust.
Frontend must use Vue, Vite, Axios, shadcn-vue, and Tailwind CSS v4.
Database must use PostgreSQL or MariaDB.
System must support REST API.
System must support auditability.
System must support role-based access control.
System must be deployable using containers.
```

---

# 3. Stakeholders

| Stakeholder | Interest |
|---|---|
| Business Users | Easy document submission and approval |
| Management | Visibility, control, reports, reduced delays |
| Finance | Approval control, budget validation, audit evidence |
| HR | Employee lifecycle document processing |
| Procurement | Vendor and purchase workflows |
| Legal | Contract approval and signed document management |
| IT | Security, integration, deployment, maintainability |
| Compliance | Audit trail, retention, access control |
| Developers | Rapid configuration and extensibility |

---

# 4. Functional Requirements

Requirements are labeled with IDs and initial priority.

Priority:

```text
Must   = Required for the 1.0 release; not tradeable against Should or Could work
Should = Important; may follow the release that first needs it
Could  = Optional or future phase
```

**Priority is not the MVP gate.** §9 is: a requirement is MVP scope if and only if it is named by an acceptance criterion there. The two are deliberately independent — some `Must` requirements (the rule engines, the dashboard) are required for 1.0 but are not part of the minimum viable release, and one requirement named by §9 (FR-API-004, OpenAPI documentation) was `Should` until v0.5 raised it. When planning, read §9 for *what ships first* and this column for *what may be cut*.

---

## 4.1 Authentication and Session Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-AUTH-001 | The system shall allow users to login using username/email and password | Must |
| FR-AUTH-002 | The system shall hash passwords using a strong password hashing algorithm such as Argon2 | Must |
| FR-AUTH-003 | The system shall issue access tokens and refresh tokens or secure session tokens | Must |
| FR-AUTH-004 | The system shall allow users to logout | Must |
| FR-AUTH-005 | The system shall allow users to change password | Should |
| FR-AUTH-006 | The system shall support forgot password and reset password flow | Should |
| FR-AUTH-007 | The system should support external SSO/OAuth2/OpenID Connect | Could |
| FR-AUTH-008 | The system shall record login activity in audit log | Must |

Note: FR-AUTH-007 is planned for a later phase.

---

## 4.2 User and Role Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-IDM-001 | The system shall manage users | Must |
| FR-IDM-002 | The system shall manage roles | Must |
| FR-IDM-003 | The system shall assign roles to users | Must |
| FR-IDM-004 | The system shall maintain the permission catalogue that authorization checks resolve against | Must |
| FR-IDM-005 | The system shall support role-permission mapping | Must |
| FR-IDM-006 | The system shall support user delegation | Should |
| FR-IDM-007 | The system shall support user status active/inactive | Must |
| FR-IDM-008 | The system shall support assigning users to a department | Should |
| FR-IDM-009 | The system shall support multi-tenant user isolation if multi-tenant mode is enabled | Should |

Note: FR-IDM-004 read "the system shall manage permissions" until v0.6, which was taken to promise administrator CRUD over permission rows. It does not. A permission is an identifier the code checks — `identity:user:create` — so the set of meaningful permissions is fixed by the code, and a row an administrator invents at runtime is inert while a check whose row an administrator deletes becomes ungrantable. The catalogue is therefore **system-defined**: seeded by migration for core modules, and extended at installation time from a plugin's manifest (FR-PLG-005; `plugin_permissions`, [Database Schema](../design/02.%20Database%20Schema.md) §13.4). Administrators read it and decide which of its entries each role holds, which is FR-IDM-005. Recorded as decision **D-6**.

Note: FR-IDM-008 read "department and position management" until v0.7, which duplicated FR-ORG-002 (departments) and FR-ORG-003 (positions) — three requirements over one existing table, `departments`, with no way to tell which of them an implementation had satisfied. Positions have no table at all; FR-ORG-003 is `Could` and unscheduled. The organization requirements own the entities; identity owns only the edge from a user to a department, which is the half that lives on `users.department_id` rather than on `departments`. Recorded as decision **D-8**.

Note: FR-IDM-009 is conditional — it obliges isolation *if multi-tenant mode is enabled* — and until v0.8 no deployment could enable it. Identity queries were tenant-scoped from Sprint 3, but nothing told a client the deployment's mode, so `KELIR_MULTI_TENANT` demanded a tenant code the login form had no field for and the backend refused to start with the mode on (decision **D-7**, v0.7).

**As of v0.8 the mode runs, and the requirement is delivered.** A tenant is named in the sign-in body, resolved once, and carried in the access token's `tenant_id` claim, which every downstream query filters by — that is per-request resolution for every request after the first. The client learns which mode it is talking to from `GET /deployment`, unauthenticated, because the login form needs the answer before it has credentials. The boot guard is gone. Two questions D-7 left open are answered rather than deferred: **roles are tenant-scoped** (each tenant has its own `ROLE-ADMIN`; the permission catalogue stays global), and **tenant administration is performed only from the deployment's default tenant**, which is what stops a tenant's own administrator creating more tenants. Recorded as decision **D-18**, superseding **D-7**.

---

## 4.3 Organization Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-ORG-001 | The system shall manage tenants | Should |
| FR-ORG-002 | The system shall manage departments | Should |
| FR-ORG-003 | The system shall manage positions | Could |
| FR-ORG-004 | The system shall manage workgroups | Could |
| FR-ORG-005 | The system shall support organizational hierarchy | Could |

FR-ORG-002 is the sole administrative surface for departments and FR-ORG-003 for positions, as of v0.7 (decision **D-8**); FR-IDM-008 covers only assigning a user to a department.

Note: FR-ORG-001 is delivered as of v0.8 and is one piece of work with FR-IDM-009 — decision **D-18** took them together, because a surface that creates tenants nobody can sign in to is not tenant management. Two properties of it are requirements-level rather than design detail. **Creating a tenant creates its first administrator in the same transaction**, so the row this surface produces is one a person can reach; giving a new tenant its first account is not the first-run bootstrap's job, which is a deployment-wide switch. And **a tenant may be administered only from the deployment's default tenant**, which is the boundary that keeps FR-IDM-002 (a tenant manages its own roles) from becoming a way to mint tenants.

---

## 4.4 Master Data Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-MDM-001 | The system shall manage party master data (persons and party groups) using a unified Party model | Must |
| FR-MDM-002 | The system shall support party roles (supplier, customer, employee, contact) with role-specific profiles | Must |
| FR-MDM-003 | The system shall support party identifications, relationships, statuses, and contact mechanisms | Must |
| FR-MDM-004 | The system shall manage facility master data | Must |
| FR-MDM-005 | The system shall manage product master data | Should |
| FR-MDM-006 | The system shall manage service master data | Should |
| FR-MDM-007 | The system shall support active/inactive status for master data | Must |
| FR-MDM-008 | The system shall support search, filter, and pagination for master data lists | Must |
| FR-MDM-009 | The system shall record master data changes in audit log | Must |
| FR-MDM-010 | The system shall allow master data changes through controlled document workflows | Should |
| FR-MDM-011 | The system shall store external source references for synchronized master data | Should |

Note: supplier, customer, and employee master data follow the OFBiz-style Party model defined in docs/architectures/05. Core - Master Data - Party.md — a party is a person or party group holding one or more roles, each with a role-specific profile. Facility, product, and service remain dedicated master-data entities.

---

## 4.5 Rapid Application Development Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-RAD-001 | The system shall support entity definitions | Must |
| FR-RAD-002 | The system shall support form definitions | Must |
| FR-RAD-003 | The system shall support list definitions | Must |
| FR-RAD-004 | The system shall support menu definitions | Should |
| FR-RAD-005 | The system shall support field definitions | Must |
| FR-RAD-006 | The system shall support validation rules | Must |
| FR-RAD-007 | The system shall support lookup fields linked to master data | Must |
| FR-RAD-008 | The system shall allow new document types to be created through configuration | Must |
| FR-RAD-009 | The system shall allow workflow to be assigned to document type | Must |
| FR-RAD-010 | The system shall support dynamic rendering of forms and lists in frontend | Must |
| FR-RAD-011 | The system shall support conditional field visibility | Should |
| FR-RAD-012 | The system shall support metadata versioning | Could |

---

## 4.6 Document Type Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-DTYPE-001 | The system shall define document types | Must |
| FR-DTYPE-002 | The system shall link document type to form definition | Must |
| FR-DTYPE-003 | The system shall link document type to workflow definition | Must |
| FR-DTYPE-004 | The system shall support document numbering rules | Must |
| FR-DTYPE-005 | The system shall support attachment rules | Should |
| FR-DTYPE-006 | The system shall support metadata rules | Should |
| FR-DTYPE-007 | The system shall support document retention policy reference | Could |
| FR-DTYPE-008 | The system shall support document security level | Should |

---

## 4.7 Document Management Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-DOC-001 | The system shall allow users to create documents based on document type | Must |
| FR-DOC-002 | The system shall allow users to edit draft documents | Must |
| FR-DOC-003 | The system shall allow users to submit documents | Must |
| FR-DOC-004 | The system shall generate document number according to numbering rule | Must |
| FR-DOC-005 | The system shall store document form data | Must |
| FR-DOC-006 | The system shall store document metadata | Must |
| FR-DOC-007 | The system shall maintain document status | Must |
| FR-DOC-008 | The system shall maintain document version history | Should |
| FR-DOC-009 | The system shall allow document cancellation before completion if permitted | Should |
| FR-DOC-010 | The system shall allow document archive after completion | Should |
| FR-DOC-011 | The system shall link document to master data entity when applicable | Must |
| FR-DOC-012 | The system shall link document to workflow process instance | Must |
| FR-DOC-013 | The system shall support document search and filter | Must |
| FR-DOC-014 | The system shall support document detail workspace | Must |

---

## 4.8 Workflow Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-WF-001 | The system shall define workflow definitions | Must |
| FR-WF-002 | The system shall support workflow states | Must |
| FR-WF-003 | The system shall support transitions between states | Must |
| FR-WF-004 | The system shall support user tasks | Must |
| FR-WF-005 | The system shall support system tasks | Should |
| FR-WF-006 | The system shall support approve action | Must |
| FR-WF-007 | The system shall support reject action | Must |
| FR-WF-008 | The system shall support return action | Should |
| FR-WF-009 | The system shall support delegate action | Should |
| FR-WF-010 | The system shall support escalation rules | Could |
| FR-WF-011 | The system shall support task due date | Should |
| FR-WF-012 | The system shall record workflow history | Must |
| FR-WF-013 | The system shall update document status based on workflow transition | Must |
| FR-WF-014 | The system shall support workflow variables | Must |
| FR-WF-015 | The system shall support conditional routing | Should |
| FR-WF-016 | The system should support parallel approval | Could |

Note: FR-WF-016 is planned for a later phase.

---

## 4.9 Task Inbox Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-TASK-001 | The system shall show tasks assigned to current user | Must |
| FR-TASK-002 | The system shall show tasks assigned to user roles | Must |
| FR-TASK-003 | The system shall allow user to open task detail | Must |
| FR-TASK-004 | The system shall allow user to approve task (see FR-WF-006 — same capability surfaced in the workflow area) | Must |
| FR-TASK-005 | The system shall allow user to reject task (see FR-WF-007 — same capability surfaced in the workflow area) | Must |
| FR-TASK-006 | The system shall allow user to add comment during approval | Must |
| FR-TASK-007 | The system shall show overdue tasks | Should |
| FR-TASK-008 | The system shall allow task delegation | Should |
| FR-TASK-009 | The system shall show completed tasks | Should |

---

## 4.10 Attachment Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-ATT-001 | The system shall allow file upload to document | Must |
| FR-ATT-002 | The system shall allow file download | Must |
| FR-ATT-003 | The system shall store attachment metadata | Must |
| FR-ATT-004 | The system shall validate file size | Must |
| FR-ATT-005 | The system shall validate file type | Must |
| FR-ATT-006 | The system shall support attachment category | Should |
| FR-ATT-007 | The system shall support attachment versioning | Could |
| FR-ATT-008 | The system shall record attachment activity | Must |
| FR-ATT-009 | The system shall support attachment deletion with soft delete | Should |
| FR-ATT-010 | The system shall support external document references | Should |

Note: FR-ATT-004 — the maximum file size is configurable, with a default limit of 25 MB per file. Baselined 2026-08-11.

---

## 4.11 Comment Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-CMT-001 | The system shall allow users to add comments to document | Must |
| FR-CMT-002 | The system shall support threaded replies | Should |
| FR-CMT-003 | The system shall support comment edit if permitted | Should |
| FR-CMT-004 | The system shall support comment delete if permitted | Should |
| FR-CMT-005 | The system shall support comment resolve status | Could |
| FR-CMT-006 | The system shall support user mention | Could |
| FR-CMT-007 | The system shall record comment activity | Must |

---

## 4.12 Activity and Audit Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-ACT-001 | The system shall record activity events for document lifecycle | Must |
| FR-ACT-002 | The system shall record activity events for attachments | Must |
| FR-ACT-003 | The system shall record activity events for comments | Must |
| FR-ACT-004 | The system shall record activity events for workflow actions | Must |
| FR-ACT-005 | The system shall provide activity timeline per document | Must |
| FR-AUD-001 | The system shall record formal audit events | Must |
| FR-AUD-002 | The system shall store old and new values for all create, update, delete, status-transition, permission, and configuration changes | Should |
| FR-AUD-003 | The system shall prevent audit records from being modified | Must |
| FR-AUD-004 | The system shall provide audit search for authorized users | Must |
| FR-AUD-005 | The system shall record IP address and actor for audit events | Should |

---

## 4.13 Notification Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-NTF-001 | The system shall generate in-app notifications | Must |
| FR-NTF-002 | The system shall notify users when task is assigned | Must |
| FR-NTF-003 | The system shall notify users when document is approved or rejected | Must |
| FR-NTF-004 | The system shall support email notification | Should |
| FR-NTF-005 | The system shall support notification templates | Should |
| FR-NTF-006 | The system shall support reminder for due tasks | Could |
| FR-NTF-007 | The system shall support escalation notification | Could |

---

## 4.14 Reporting and Dashboard Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-RPT-001 | The system shall provide dashboard summary | Must |
| FR-RPT-002 | The system shall show pending tasks | Must |
| FR-RPT-003 | The system shall show recent documents | Must |
| FR-RPT-004 | The system shall show document status summary | Should |
| FR-RPT-005 | The system shall show overdue tasks | Should |
| FR-RPT-006 | The system shall provide approval time report | Could |
| FR-RPT-007 | The system shall provide workload by department report | Could |
| FR-RPT-008 | The system shall support export to CSV or Excel | Could |

---

## 4.15 Search Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-SRH-001 | The system shall support document search (see FR-DOC-013 — same capability surfaced in the document area) | Must |
| FR-SRH-002 | The system shall support master data search (see FR-MDM-008 — same capability surfaced in the master data area) | Must |
| FR-SRH-003 | The system shall support task search | Should |
| FR-SRH-004 | The system shall support global search | Could |

---

## 4.16 Integration Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-INT-001 | The system shall provide integration core module | Should |
| FR-INT-002 | The system shall support outbound REST API calls | Should |
| FR-INT-003 | The system shall support inbound REST APIs | Should |
| FR-INT-004 | The system shall support webhook receiver | Could |
| FR-INT-005 | The system shall support webhook publisher | Could |
| FR-INT-006 | The system shall support integration logs | Should |
| FR-INT-007 | The system shall support retry for failed integration | Could |
| FR-INT-008 | The system shall support master data synchronization | Could |
| FR-INT-009 | The system shall support file exchange | Could |
| FR-INT-010 | The system shall support external document reference | Should |

---

## 4.17 Plugin and Extension Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-PLG-001 | The system shall support plugin registry | Could |
| FR-PLG-002 | The system shall support plugin manifest | Could |
| FR-PLG-003 | The system shall support plugin enable/disable | Could |
| FR-PLG-004 | The system shall support plugin settings | Could |
| FR-PLG-005 | The system shall support plugin permissions | Could |
| FR-PLG-006 | The system shall support UI extension points | Could |
| FR-PLG-007 | The system shall support backend extension points | Could |
| FR-PLG-008 | The system shall audit plugin installation and configuration | Could |

---

## 4.18 API Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-API-001 | The system shall provide REST API | Must |
| FR-API-002 | The system shall use JSON format | Must |
| FR-API-003 | The system shall version APIs under /api/v1 | Must |
| FR-API-004 | The system shall provide OpenAPI documentation | Must |
| FR-API-005 | The system shall provide standard error response format | Must |
| FR-API-006 | The system shall support pagination | Must |
| FR-API-007 | The system shall support sorting and filtering | Should |
| FR-API-008 | The system shall protect APIs using authentication and authorization | Must |

---

# 5. Non-Functional Requirements

## 5.1 Performance

| ID | Requirement |
|---|---|
| NFR-PERF-001 | List views should load within 1 second and detail views within 2 seconds at P95, with 10,000 documents and 50 concurrent users |
| NFR-PERF-002 | API endpoints should support pagination to avoid large payloads |
| NFR-PERF-003 | Workflow transitions should complete without blocking UI unnecessarily |
| NFR-PERF-004 | File upload should support streaming where possible |

Initial target:

```text
API list response: under 1 second for typical queries
Document detail: under 2 seconds for typical document
File upload: depends on network and file size
```

---

## 5.2 Scalability

| ID | Requirement |
|---|---|
| NFR-SCA-001 | Backend should be stateless where possible |
| NFR-SCA-002 | File storage should be externalized from application server |
| NFR-SCA-003 | Background jobs should be separated from request handling |
| NFR-SCA-004 | Architecture should allow future service separation |

---

## 5.3 Availability

| ID | Requirement |
|---|---|
| NFR-AVA-001 | System should support containerized deployment |
| NFR-AVA-002 | System should support health checks |
| NFR-AVA-003 | System should support graceful shutdown |
| NFR-AVA-004 | Production deployments should achieve 99.5% monthly uptime, with RPO ≤ 24 hours, RTO ≤ 4 hours, and daily automated backups |

---

## 5.4 Security

| ID | Requirement |
|---|---|
| NFR-SEC-001 | Passwords must be hashed using strong algorithm |
| NFR-SEC-002 | Tokens must have expiration |
| NFR-SEC-003 | APIs must enforce authentication and authorization |
| NFR-SEC-004 | Sensitive data must be masked in logs |
| NFR-SEC-005 | File uploads must be validated |
| NFR-SEC-006 | Audit trail must be protected from unauthorized modification |
| NFR-SEC-007 | Secrets must not be hardcoded |
| NFR-SEC-008 | Authentication endpoints must be rate limited: 5 failed logins trigger a 15-minute lockout |
| NFR-SEC-009 | Attachments and backups must be encrypted at rest |
| NFR-SEC-010 | Data in transit must use TLS 1.2 or higher |

---

## 5.5 Auditability

| ID | Requirement |
|---|---|
| NFR-AUD-001 | All create, update, delete, status-transition, permission, and configuration changes must be auditable |
| NFR-AUD-002 | Audit log must include actor, timestamp, action, and object |
| NFR-AUD-003 | Document approval history must be traceable |
| NFR-AUD-004 | Master data changes must be traceable |

---

## 5.6 Maintainability

| ID | Requirement |
|---|---|
| NFR-MNT-001 | Backend should use modular structure |
| NFR-MNT-002 | Frontend should use feature-based structure |
| NFR-MNT-003 | Code should follow Rust and Vue best practices |
| NFR-MNT-004 | Database migrations should be versioned |
| NFR-MNT-005 | API documentation should be generated or maintained |

---

## 5.7 Usability

| ID | Requirement |
|---|---|
| NFR-USE-001 | UI should be responsive for desktop and tablet |
| NFR-USE-002 | UI should support clear navigation and breadcrumbs |
| NFR-USE-003 | Document workspace should show all relevant tabs |
| NFR-USE-004 | Approval actions should be simple and clear |
| NFR-USE-005 | Error messages should be user-friendly |

---

## 5.8 Compatibility

| ID | Requirement |
|---|---|
| NFR-CMP-001 | Frontend should support modern browsers |
| NFR-CMP-002 | Backend should support PostgreSQL as primary database |
| NFR-CMP-003 | MariaDB support may be provided with limitations |
| NFR-CMP-004 | API should be consumable by web, mobile, or external clients |

---

# 6. External Interface Requirements

## 6.1 User Interface

The frontend shall provide:

```text
Login page
Dashboard
Document list
Document create/edit
Document workspace
Task inbox
Master data management
Admin configuration
Audit and activity viewer
Notification center
Settings
```

---

## 6.2 API Interface

The backend shall provide REST APIs for:

```text
Authentication
Users
Roles
Organization
Master data
Document types
Documents
Workflow definitions
Process instances
Tasks
Attachments
Comments
Activity
Audit
Notifications
Reports
Integration
Plugins
```

---

## 6.3 Database Interface

The system shall use:

```text
PostgreSQL as primary database
SQL migration scripts
Connection pooling
Repository pattern
```

---

## 6.4 File Storage Interface

The system shall support:

```text
Local filesystem for development
S3-compatible object storage for production
Attachment metadata stored in database
File content stored outside core database
```

---

## 6.5 Integration Interface

The system shall support future integration through:

```text
REST APIs
Webhooks
Event outbox
Scheduled jobs
File exchange
Message queue, optional
```

---

# 7. Data Requirements

Initial core data groups:

```text
Identity and organization
Roles and permissions
Master data
Document type configuration
RAD metadata
Documents
Document versions
Document metadata
Attachments
Comments
Workflow definitions
Workflow instances
Workflow tasks
Workflow history
Activity events
Audit events
Notifications
Integration logs
Plugin registry
```

---

# 8. Security Requirements Summary

```text
Authentication required for protected APIs
RBAC enforced on backend and frontend
Password hashing using Argon2 or equivalent
JWT/session expiration
Refresh token rotation if JWT is used
Secure cookie option if session-based
Input validation using schema validation
Parameterized SQL queries only
File upload validation
Audit trail protection
Secrets stored outside source code
HTTPS enforced in production
```

---

# 9. Acceptance Criteria for MVP

The MVP is considered acceptable if:

```text
Users can login and logout.
Administrators can manage users and roles.
Administrators can configure master data.
Administrators can configure document types.
Users can create documents from configured document type.
Users can upload attachments.
Users can submit documents.
Workflow engine creates tasks.
Approvers can approve or reject tasks.
Document status changes automatically.
Comments can be added.
Activity log records actions.
Audit log records approvals and status changes.
REST APIs are available and documented.
Application runs using Docker Compose.
```

---

# 10. Out of Scope for Initial Version

The following are not required for initial MVP unless specifically prioritized:

```text
Full BPMN 2.0 engine
Dynamic third-party plugin marketplace
Drag-and-drop workflow designer
Drag-and-drop form builder with advanced logic
Mobile native application
Real-time WebSocket collaboration
Advanced AI classification
Complex ABAC policies
Multi-region high availability
Payment processing
E-signature legal integration
Machine learning analytics
```

---

# Part 2: Solution Blueprint (moved)

The "Kelir Solution Blueprint" formerly embedded here as Document 2 has been moved to the
System Design Document: `docs/design/01. System Design Document.md` (SDD v0.1).

The SDD covers: architecture style and overview, technology stack, backend/frontend structure,
database design, workflow engine, RAD engine, integration, plugins, security, API, deployment,
the development roadmap, traceability, and risks.
