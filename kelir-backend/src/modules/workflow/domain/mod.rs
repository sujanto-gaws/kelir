//! Workflow domain types (FR-WF-001..004, 006, 007, 013, 014).

pub mod approval_time;
pub mod definition;
pub mod graph;
pub mod instance;
pub mod jwss;
pub mod task;
pub mod task_type;

pub use approval_time::ApprovalTime;
pub use definition::{
    validate_create, validate_update, CreateWorkflowRequest, DefinitionNamingRole,
    UpdateWorkflowRequest, WorkflowDefinition, WorkflowDefinitionStatus, WorkflowDefinitionSummary,
};
pub use graph::{
    AssigneeType, AssignmentRule, Graph, State, TaskSpec, Transition, TransitionAction,
};
pub use instance::{
    InstanceOutcome, InstanceStatus, WorkflowHistoryEntry, WorkflowInstance, WorkflowVariable,
};
pub use task::{
    Assignment, DecisionAction, DecisionRequest, DelegateRequest, TaskStatus, WorkflowTask,
};
pub use task_type::{TaskType, TaskTypeRefusal};
