# Kelir Initial Documents

Below are the initial documents for **Kelir**:

1. **Kelir Software Requirements Specification (SRS)**
2. **Kelir Solution Blueprint**

These documents are intended as version **0.1** and can be expanded into formal project documentation before development starts.

---

# Document 1: Kelir Software Requirements Specification

## Document Control

| Item | Detail |
|---|---|
| Document Name | Kelir Software Requirements Specification |
| Framework Name | Kelir |
| Version | 0.1 |
| Status | Initial Draft |
| Date | 2026-08-05 |
| Document Type | SRS |
| Architecture Style | Full-stack document-based workflow platform |
| Backend | Rust |
| Frontend | Vue + Vite + Axios + shadcn-vue + Tailwind CSS v4 |
| Database | PostgreSQL, optional MariaDB compatibility |

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
| Master Data | Core reference data such as employee, supplier, customer, facility, product |
| RAD | Rapid Application Development |
| Plugin | Extension that adds features to Kelir |
| Integration | Connection with external systems |
| Audit Trail | Immutable record of business and system actions |
| RBAC | Role-Based Access Control |
| ABAC | Attribute-Based Access Control |
| MDM | Master Data Management |

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
| Tenant Administrator | Manages configuration for a tenant |
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
Must = Required for MVP
Should = Important but can follow shortly after MVP
Could = Optional or future phase
```

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
| FR-AUTH-007 | The system shall support external SSO/OAuth2/OpenID Connect in later phase | Could |
| FR-AUTH-008 | The system shall record login activity in audit log | Must |

---

## 4.2 User and Role Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-IDM-001 | The system shall manage users | Must |
| FR-IDM-002 | The system shall manage roles | Must |
| FR-IDM-003 | The system shall assign roles to users | Must |
| FR-IDM-004 | The system shall manage permissions | Must |
| FR-IDM-005 | The system shall support role-permission mapping | Must |
| FR-IDM-006 | The system shall support user delegation | Should |
| FR-IDM-007 | The system shall support user status active/inactive | Must |
| FR-IDM-008 | The system shall support department and position management | Should |
| FR-IDM-009 | The system shall support multi-tenant user isolation if multi-tenant mode is enabled | Should |

---

## 4.3 Organization Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-ORG-001 | The system shall manage tenants | Should |
| FR-ORG-002 | The system shall manage departments | Should |
| FR-ORG-003 | The system shall manage positions | Could |
| FR-ORG-004 | The system shall manage workgroups | Could |
| FR-ORG-005 | The system shall support organizational hierarchy | Could |

---

## 4.4 Master Data Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-MDM-001 | The system shall manage supplier master data | Must |
| FR-MDM-002 | The system shall manage customer master data | Must |
| FR-MDM-003 | The system shall manage employee master data | Must |
| FR-MDM-004 | The system shall manage facility master data | Must |
| FR-MDM-005 | The system shall manage product master data | Should |
| FR-MDM-006 | The system shall manage service master data | Should |
| FR-MDM-007 | The system shall support active/inactive status for master data | Must |
| FR-MDM-008 | The system shall support search, filter, and pagination for master data lists | Must |
| FR-MDM-009 | The system shall record master data changes in audit log | Must |
| FR-MDM-010 | The system shall allow master data changes through controlled document workflows | Should |
| FR-MDM-011 | The system shall store external source references for synchronized master data | Should |

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
| FR-WF-016 | The system shall support parallel approval in later phase | Could |

---

## 4.9 Task Inbox Requirements

| ID | Requirement | Priority |
|---|---|---|
| FR-TASK-001 | The system shall show tasks assigned to current user | Must |
| FR-TASK-002 | The system shall show tasks assigned to user roles | Must |
| FR-TASK-003 | The system shall allow user to open task detail | Must |
| FR-TASK-004 | The system shall allow user to approve task | Must |
| FR-TASK-005 | The system shall allow user to reject task | Must |
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
| FR-AUD-002 | The system shall store old and new values for important changes | Should |
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
| FR-SRH-001 | The system shall support document search | Must |
| FR-SRH-002 | The system shall support master data search | Must |
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
| FR-API-004 | The system shall provide OpenAPI documentation | Should |
| FR-API-005 | The system shall provide standard error response format | Must |
| FR-API-006 | The system shall support pagination | Must |
| FR-API-007 | The system shall support sorting and filtering | Should |
| FR-API-008 | The system shall protect APIs using authentication and authorization | Must |

---

# 5. Non-Functional Requirements

## 5.1 Performance

| ID | Requirement |
|---|---|
| NFR-PERF-001 | List pages should load within acceptable time for normal dataset |
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

---

## 5.5 Auditability

| ID | Requirement |
|---|---|
| NFR-AUD-001 | All important business actions must be auditable |
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

# Document 2: Kelir Solution Blueprint

## Document Control

| Item | Detail |
|---|---|
| Document Name | Kelir Solution Blueprint |
| Framework Name | Kelir |
| Version | 0.1 |
| Status | Initial Draft |
| Date | 2026-08-05 |
| Document Type | Architecture Blueprint |
| Backend | Rust |
| Frontend | Vue + Vite + Axios + shadcn-vue + Tailwind CSS v4 |
| Database | PostgreSQL, optional MariaDB |

---

# 1. Blueprint Purpose

This document defines the initial architecture and technical blueprint for **Kelir**.

It describes:

```text
Architecture style
Technology stack
Backend structure
Frontend structure
Database design approach
Workflow engine design
RAD engine design
Integration design
Plugin/extension design
Security design
Deployment design
Development roadmap
```

---

# 2. Architecture Style

Kelir will use:

```text
Modular Monolith Backend
Single Page Application Frontend
REST API
Relational Database
Object/File Storage
Background Worker
Event Outbox
```

Initial architecture:

```text
Frontend SPA
    ↓ HTTPS/JSON
Rust Backend API
    ↓
PostgreSQL Database
File/Object Storage
Background Worker / Scheduler
```

Future evolution:

```text
Microservices if needed
Message queue
External workflow engine
Plugin services
API gateway
Event streaming
```

---

# 3. High-Level Architecture

```text
+---------------------------------------------------------------+
|                      Kelir Frontend                      |
|                                                               |
|  Vue 3 + Vite + TypeScript                                    |
|  Pinia + Vue Router                                           |
|  Axios + shadcn-vue + Tailwind CSS v4                         |
|                                                               |
|  Dynamic Form Renderer                                        |
|  Dynamic List Renderer                                        |
|  Document Workspace                                           |
|  Task Inbox                                                   |
|  Admin Console                                                |
+---------------------------------------------------------------+
                              |
                              | REST / JSON
                              v
+---------------------------------------------------------------+
|                       Kelir Backend                      |
|                                                               |
|  Rust + Axum + Tokio                                          |
|                                                               |
|  Authentication                                               |
|  Authorization                                                |
|  API Controllers                                              |
|  Application Services                                         |
|  Domain Modules                                               |
|  Workflow Engine                                              |
|  RAD Engine                                                   |
|  Integration Layer                                            |
|  Plugin Runtime                                               |
|  Event Dispatcher                                             |
+---------------------------------------------------------------+
                              |
        +---------------------+---------------------+
        |                     |                     |
        v                     v                     v
+--------------+      +----------------+     +----------------+
| PostgreSQL   |      | Object Storage |     | Worker / Queue |
| or MariaDB   |      | Local / MinIO  |     | Outbox / Jobs  |
|              |      | / S3           |     |                |
+--------------+      +----------------+     +----------------+
```

---

# 4. Technology Stack Blueprint

## 4.1 Backend Stack

| Layer | Technology |
|---|---|
| Language | Rust |
| Web Framework | Axum |
| Async Runtime | Tokio |
| Middleware | Tower / Tower-HTTP |
| Serialization | Serde |
| Validation | Validator or Zod-like server validation |
| Database Access | SQLx |
| Migration | SQLx Migrate |
| Authentication | JWT or secure session |
| Password Hashing | Argon2 |
| API Docs | Utoipa / OpenAPI |
| Logging | Tracing |
| Error Handling | ThisError / Anyhow |
| Background Jobs | Tokio tasks + outbox, later queue |
| Storage | Local, MinIO, S3-compatible |

---

## 4.2 Frontend Stack

| Layer | Technology |
|---|---|
| Framework | Vue 3 |
| Build Tool | Vite |
| Language | TypeScript |
| State | Pinia |
| Router | Vue Router |
| HTTP Client | Axios |
| UI Components | shadcn-vue |
| Styling | Tailwind CSS v4 |
| Form Validation | VeeValidate + Zod |
| Utilities | VueUse |
| Icons | Lucide Vue Next |
| Server State | TanStack Query Vue, optional |

---

## 4.3 Database Strategy

Primary database:

```text
PostgreSQL
```

Reason:

```text
JSONB support
GIN indexing
Full-text search
Strong integrity
Row-level security potential
Better fit for metadata and audit logs
```

Optional:

```text
MariaDB
```

MariaDB support requires:

```text
SQLx MySQL dialect
Avoid PostgreSQL-specific JSONB features
Test migration compatibility
Use generated columns if indexing JSON fields
```

---

# 5. Backend Blueprint

## 5.1 Backend Layering

```text
HTTP Layer
    ↓
Application Layer
    ↓
Domain Layer
    ↓
Infrastructure Layer
```

---

## 5.2 Backend Modules

```text
BhuvarlokaCoreModule
BhuvarlokaConfigModule
BhuvarlokaDatabaseModule
BhuvarlokaHealthModule
BhuvarlokaSecurityModule
BhuvarlokaIdentityModule
BhuvarlokaRolePermissionModule
BhuvarlokaOrganizationModule
BhuvarlokaMasterDataModule
BhuvarlokaRadModule
BhuvarlokaDocumentTypeModule
BhuvarlokaDocumentModule
BhuvarlokaWorkflowModule
BhuvarlokaTaskInboxModule
BhuvarlokaAttachmentModule
BhuvarlokaStorageModule
BhuvarlokaCommentModule
BhuvarlokaActivityModule
BhuvarlokaAuditModule
BhuvarlokaNotificationModule
BhuvarlokaReportingModule
BhuvarlokaSearchModule
BhuvarlokaSchedulerModule
BhuvarlokaEventModule
BhuvarlokaIntegrationModule
BhuvarlokaPluginModule
BhuvarlokaApiModule
BhuvarlokaOpenApiModule
```

---

## 5.3 Backend Folder Blueprint

Initial simple structure:

```text
kelir-backend/
├── Cargo.toml
├── migrations/
│   ├── 0001_core.sql
│   ├── 0002_identity.sql
│   ├── 0003_master_data.sql
│   ├── 0004_rad.sql
│   ├── 0005_document.sql
│   ├── 0006_workflow.sql
│   ├── 0007_attachment.sql
│   ├── 0008_comment.sql
│   ├── 0009_activity_audit.sql
│   ├── 0010_notification.sql
│   ├── 0011_integration.sql
│   └── 0012_plugin.sql
│
└── src/
    ├── main.rs
    ├── config.rs
    ├── error.rs
    ├── router.rs
    ├── db.rs
    ├── health.rs
    ├── middleware/
    ├── modules/
    │   ├── auth/
    │   ├── identity/
    │   ├── roles/
    │   ├── organization/
    │   ├── master_data/
    │   ├── rad/
    │   ├── document_type/
    │   ├── document/
    │   ├── workflow/
    │   ├── task_inbox/
    │   ├── attachment/
    │   ├── comment/
    │   ├── activity/
    │   ├── audit/
    │   ├── notification/
    │   ├── reporting/
    │   ├── search/
    │   ├── integration/
    │   └── plugin/
    └── utils/
```

Later can evolve into Rust workspace crates:

```text
crates/
├── api
├── app
├── domain
├── infrastructure
├── workflow
├── rad
├── integration
├── plugin
└── security
```

---

# 6. Frontend Blueprint

## 6.1 Frontend Architecture

```text
App Shell
    ↓
Layout
    ↓
Router
    ↓
Feature Modules
    ↓
API Client
    ↓
Backend REST API
```

---

## 6.2 Frontend Modules

```text
BhuvarlokaAppShellModule
BhuvarlokaLayoutModule
BhuvarlokaRouterModule
BhuvarlokaStoreModule
BhuvarlokaApiClientModule
BhuvarlokaAuthFeatureModule
BhuvarlokaDashboardFeatureModule
BhuvarlokaTaskInboxFeatureModule
BhuvarlokaDocumentFeatureModule
BhuvarlokaDocumentWorkspaceModule
BhuvarlokaAttachmentFeatureModule
BhuvarlokaCommentFeatureModule
BhuvarlokaActivityFeatureModule
BhuvarlokaAuditFeatureModule
BhuvarlokaMasterDataFeatureModule
BhuvarlokaRadRendererModule
BhuvarlokaFormEngineModule
BhuvarlokaTableModule
BhuvarlokaWorkflowFeatureModule
BhuvarlokaAdminFeatureModule
BhuvarlokaRadBuilderModule
BhuvarlokaNotificationFeatureModule
BhuvarlokaSearchFeatureModule
BhuvarlokaReportingFeatureModule
BhuvarlokaSettingsFeatureModule
BhuvarlokaUiModule
BhuvarlokaThemeModule
BhuvarlokaErrorHandlingModule
BhuvarlokaUtilityModule
BhuvarlokaTypeModule
```

---

## 6.3 Frontend Folder Blueprint

```text
kelir-frontend/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
└── src/
    ├── main.ts
    ├── App.vue
    ├── api/
    ├── components/
    ├── composables/
    ├── features/
    │   ├── auth/
    │   ├── dashboard/
    │   ├── documents/
    │   ├── workflow/
    │   ├── tasks/
    │   ├── master-data/
    │   ├── admin/
    │   ├── notifications/
    │   └── settings/
    ├── layouts/
    ├── pages/
    ├── router/
    ├── stores/
    ├── styles/
    ├── types/
    └── lib/
```

---

# 7. Database Blueprint

## 7.1 Table Groups

```text
Core and configuration
Identity and security
Organization
Master data
RAD metadata
Document type
Document
Attachment
Comment
Workflow
Activity and audit
Notification
Integration
Plugin
```

---

## 7.2 Core Tables

```text
tenants
departments
users
roles
permissions
role_permissions
user_roles
delegations
```

---

## 7.3 Master Data Tables

```text
mdm_suppliers
mdm_customers
mdm_employees
mdm_facilities
mdm_products
mdm_services
master_data_source_references
```

---

## 7.4 RAD Tables

```text
rad_entities
rad_fields
rad_forms
rad_form_sections
rad_form_fields
rad_lists
rad_list_columns
rad_list_filters
rad_menus
rad_actions
rad_validation_rules
rad_lookup_definitions
```

---

## 7.5 Document Tables

```text
document_types
document_type_workflows
document_type_numbering_rules
document_type_attachment_rules
documents
document_versions
document_metadata
document_status_history
document_relations
```

---

## 7.6 Workflow Tables

```text
workflow_definitions
workflow_states
workflow_transitions
workflow_instances
workflow_variables
workflow_tasks
workflow_task_history
approval_decisions
workflow_escalations
```

---

## 7.7 Collaboration Tables

```text
attachments
attachment_versions
attachment_categories
comments
comment_mentions
comment_attachments
```

---

## 7.8 Activity and Audit Tables

```text
activity_events
audit_events
audit_snapshots
```

---

## 7.9 Notification Tables

```text
notifications
notification_templates
notification_channels
notification_logs
```

---

## 7.10 Integration Tables

```text
external_systems
integration_endpoints
integration_credentials
integration_logs
integration_mappings
webhook_subscriptions
webhook_events
outbox_events
inbox_events
```

---

## 7.11 Plugin Tables

```text
plugins
plugin_versions
plugin_installations
plugin_permissions
plugin_settings
plugin_migrations
plugin_audit_logs
plugin_error_logs
```

---

# 8. Workflow Engine Blueprint

## 8.1 Workflow Model

Initial workflow model:

```text
Workflow Definition
    ↓
States
    ↓
Transitions
    ↓
Actions
    ↓
Process Instance
    ↓
Tasks
    ↓
History
```

---

## 8.2 Workflow States Example

```text
DRAFT
SUBMITTED
MANAGER_APPROVAL
FINANCE_APPROVAL
DIRECTOR_APPROVAL
COMPLETED
REJECTED
RETURNED
CANCELLED
ARCHIVED
```

---

## 8.3 Workflow Actions

```text
SUBMIT
APPROVE
REJECT
RETURN
DELEGATE
ESCALATE
CANCEL
COMPLETE
```

---

## 8.4 Workflow Processing Flow

```text
Document submitted
        ↓
Workflow engine selects workflow definition
        ↓
Process instance created
        ↓
First transition evaluated
        ↓
Task created if human action required
        ↓
Task assigned to user/role/group
        ↓
User completes task
        ↓
Transition evaluated
        ↓
Document status updated
        ↓
Next task created or process completed
        ↓
History and audit recorded
```

---

# 9. RAD Engine Blueprint

## 9.1 RAD Concept

```text
Entity Definition
    ↓
Form Definition
    ↓
List Definition
    ↓
Document Type
    ↓
Workflow Assignment
    ↓
UI Renderer
```

---

## 9.2 RAD Metadata Objects

```text
Entity
Field
Form
Form Section
Form Field
List
List Column
List Filter
Menu
Action
Lookup
Validation Rule
```

---

## 9.3 RAD Rendering Flow

```text
Frontend requests metadata
        ↓
Backend returns form/list/document type metadata
        ↓
Frontend FormRenderer renders form
        ↓
Frontend ListRenderer renders table
        ↓
User submits data
        ↓
Backend validates using schema
        ↓
Document or master data saved
```

---

# 10. Integration Blueprint

## 10.1 Integration Layer Components

```text
Integration Core
Connector/Adapter
Outbound API Client
Inbound API
Webhook Module
Event Bus / Outbox
Mapping Module
Scheduler
Retry Module
Secret Management
Integration Log
Integration Audit
```

---

## 10.2 Integration Flow

```text
Business event occurs
        ↓
Integration event created
        ↓
Outbox stores event
        ↓
Worker processes event
        ↓
Connector maps payload
        ↓
External system called
        ↓
Response logged
        ↓
Kelir state updated
```

---

# 11. Plugin / Extension Blueprint

## 11.1 Extension Strategy

Phase 1:

```text
Configuration-based extensions
```

Phase 2:

```text
Compiled-in official plugins
```

Phase 3:

```text
Dynamic plugins or external plugin services
```

---

## 11.2 Plugin Management Components

```text
Plugin Registry
Plugin Loader
Plugin Lifecycle Manager
Plugin Permission Manager
Plugin Hook Manager
Plugin Settings Manager
Plugin Asset Manager
Plugin Audit Manager
Plugin Manager UI
```

---

## 11.3 Plugin Extension Points

Backend:

```text
API routes
Workflow handlers
Document validators
Integration connectors
Notification channels
Storage drivers
Auth providers
Background jobs
Event subscribers
```

Frontend:

```text
Sidebar menu
Dashboard widget
Document tab
Task action
Master data tab
Admin page
Report widget
Form field renderer
Table column renderer
Theme
Localization
```

---

# 12. Security Blueprint

## 12.1 Authentication

```text
Username/email + password
Argon2 password hashing
JWT access token or secure session
Refresh token or session renewal
Logout and token invalidation
```

---

## 12.2 Authorization

```text
RBAC first
Permission format: module:resource:action
Backend enforcement
Frontend visibility control
```

Example permissions:

```text
document:create
document:read
document:update
document:submit
document:approve
attachment:upload
attachment:delete
master-data:supplier:update
audit:read
integration:manage
plugin:manage
```

---

## 12.3 Audit and Logging

```text
Activity log for user-friendly timeline
Audit log for formal compliance record
Integration log for external calls
Plugin audit log for extension actions
Security log for authentication and authorization failures
```

---

# 13. API Blueprint

## 13.1 Base API

```text
/api/v1
```

---

## 13.2 Core Endpoints

```text
POST /auth/login
POST /auth/logout
GET  /auth/me

GET  /users
POST /users
GET  /users/{id}
PUT  /users/{id}

GET  /roles
POST /roles

GET  /master-data/suppliers
GET  /master-data/customers
GET  /master-data/employees
GET  /master-data/facilities
GET  /master-data/products
GET  /master-data/services

GET  /document-types
POST /document-types

GET  /documents
POST /documents
GET  /documents/{id}
PUT  /documents/{id}
POST /documents/{id}/submit

GET  /tasks
GET  /tasks/{id}
POST /tasks/{id}/approve
POST /tasks/{id}/reject
POST /tasks/{id}/return
POST /tasks/{id}/delegate

GET  /notifications
GET  /activity
GET  /audit
```

---

## 13.3 Standard Response

```json
{
  "success": true,
  "data": {},
  "meta": {}
}
```

Standard error:

```json
{
  "success": false,
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Validation failed",
    "details": []
  }
}
```

---

# 14. Deployment Blueprint

## 14.1 Development Deployment

```text
Docker Compose
├── frontend
├── backend
├── postgres
├── mailpit
└── minio
```

---

## 14.2 Production Deployment

```text
Nginx / Reverse Proxy
├── Static frontend
└── Proxy to backend API

Backend container
PostgreSQL managed database
Object storage / MinIO / S3
Optional queue / worker
Secret management
Backup strategy
Monitoring
```

---

## 14.3 Environment Variables

Backend:

```text
BHUVARLOKA_APP_NAME
BHUVARLOKA_APP_ENV
BHUVARLOKA_DATABASE_URL
BHUVARLOKA_JWT_SECRET
BHUVARLOKA_STORAGE_DRIVER
BHUVARLOKA_SMTP_HOST
BHUVARLOKA_FRONTEND_URL
```

Frontend:

```text
VITE_BHUVARLOKA_API_BASE_URL
VITE_BHUVARLOKA_APP_TITLE
```

---

# 15. Development Roadmap

## Phase 1: Foundation

```text
Project skeleton
Docker Compose
Backend Axum skeleton
Frontend Vue skeleton
Database connection
Health endpoint
Configuration loading
Logging
Basic layout
Login page
```

Deliverable:

```text
Running Kelir skeleton
```

---

## Phase 2: Authentication and User Management

```text
Users
Roles
Permissions
Login
Logout
JWT/session
Route guards
User CRUD
Role CRUD
```

Deliverable:

```text
Authenticated Kelir application
```

---

## Phase 3: Master Data

```text
Supplier
Customer
Employee
Facility
Product
Service
Master data list
Master data form
Search and pagination
Audit logging
```

Deliverable:

```text
Master data management module
```

---

## Phase 4: Document Core

```text
Document types
Document numbering
Document creation
Document editing
Document submission
Document list
Document detail
Document metadata
Document versions
```

Deliverable:

```text
Working document management module
```

---

## Phase 5: Workflow Engine

```text
Workflow definitions
Process instances
Tasks
Approve
Reject
Return
Task inbox
Workflow history
Document status synchronization
```

Deliverable:

```text
Approval workflow working end-to-end
```

---

## Phase 6: Attachments, Comments, Activity

```text
File upload
File download
Attachment list
Comments
Threaded comments
Activity timeline
Basic audit trail
```

Deliverable:

```text
Collaborative document workspace
```

---

## Phase 7: RAD Engine

```text
Entity definitions
Form definitions
List definitions
Dynamic form renderer
Dynamic list renderer
Document type builder
Menu builder
```

Deliverable:

```text
Configurable rapid application development capability
```

---

## Phase 8: Reporting and Dashboard

```text
Dashboard summary
Pending tasks
Recent documents
Document status chart
Overdue tasks
Approval reports
```

Deliverable:

```text
Management dashboard
```

---

## Phase 9: Integration and Plugin Foundation

```text
Integration core
External system registry
Integration logs
Outbox events
Plugin registry
Plugin settings
Plugin manager UI
```

Deliverable:

```text
Extensible Kelir platform foundation
```

---

# 16. Traceability Matrix

| Requirement Area | Related Blueprint Modules |
|---|---|
| Authentication | SecurityModule, AuthFeatureModule |
| Users and Roles | IdentityModule, RolePermissionModule, AdminFeatureModule |
| Master Data | MasterDataModule, MasterDataFeatureModule |
| RAD | RadModule, RadRendererModule, RadBuilderModule |
| Document Type | DocumentTypeModule, AdminFeatureModule |
| Documents | DocumentModule, DocumentFeatureModule, DocumentWorkspaceModule |
| Workflow | WorkflowModule, WorkflowFeatureModule |
| Tasks | TaskInboxModule, TaskInboxFeatureModule |
| Attachments | AttachmentModule, StorageModule, AttachmentFeatureModule |
| Comments | CommentModule, CommentFeatureModule |
| Activity | ActivityModule, ActivityFeatureModule |
| Audit | AuditModule, AuditFeatureModule |
| Notifications | NotificationModule, NotificationFeatureModule |
| Reporting | ReportingModule, DashboardFeatureModule |
| Integration | IntegrationModule, IntegrationAdminModule |
| Plugins | PluginModule, PluginManagerUIModule |

---

# 17. Risks and Mitigation

| Risk | Mitigation |
|---|---|
| Workflow engine becomes too complex | Start with lightweight state-transition engine before BPMN |
| Multi-database compatibility complexity | Use PostgreSQL first, MariaDB later with compatibility layer |
| Dynamic plugins introduce security risk | Start with configuration-based and compiled-in plugins |
| RAD metadata becomes unstable | Version metadata and validate schema strictly |
| Frontend dynamic rendering complexity | Build reusable FormRenderer and ListRenderer incrementally |
| Audit log grows large | Partition/archive audit tables and define retention policy |
| File storage grows | Use object storage and metadata tracking |
| Integration failures | Use outbox, retry, dead-letter, and integration logs |
| Permission complexity | Start with RBAC before ABAC |

---

# 18. Immediate Next Steps

The next practical steps are:

```text
1. Approve SRS v0.1 and Blueprint v0.1.
2. Finalize repository structure.
3. Initialize Kelir backend skeleton using Rust and Axum.
4. Initialize Kelir frontend skeleton using Vue, Vite, TypeScript, shadcn-vue, Tailwind CSS v4.
5. Prepare Docker Compose for PostgreSQL, backend, frontend, MinIO, and Mailpit.
6. Create initial database migrations.
7. Implement health endpoint.
8. Implement authentication.
9. Implement user and role management.
10. Implement master data CRUD.
11. Implement document core.
12. Implement workflow engine.
```

---

# 19. Final Statement

**Kelir** will be a document-based, workflow-driven, rapid application development framework with the following foundation:

```text
Rust backend
Vue frontend
PostgreSQL database
REST API
Modular monolith architecture
Metadata-driven RAD engine
Lightweight workflow engine
Attachment and collaboration features
Audit-ready document processing
Integration-ready architecture
Plugin-ready extension model
```

The SRS and Blueprint above provide the initial baseline for starting design refinement, repository initialization, database migration design, API design, and development implementation.
