//! Workflow domain types (FR-WF-001..004, 006, 007, 013, 014).

pub mod definition;
pub mod graph;
pub mod instance;
pub mod jwss;
pub mod task;

pub use definition::{
    validate_create, validate_update, CreateWorkflowRequest, UpdateWorkflowRequest,
    WorkflowDefinition, WorkflowDefinitionStatus, WorkflowDefinitionSummary,
};
pub use graph::{
    AssigneeType, AssignmentRule, Graph, State, TaskSpec, Transition, TransitionAction,
};
pub use instance::{InstanceOutcome, InstanceStatus, WorkflowInstance, WorkflowVariable};
pub use task::{Assignment, DecisionAction, DecisionRequest, TaskStatus, WorkflowTask};
