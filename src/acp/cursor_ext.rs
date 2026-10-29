//! Cursor ACP extension methods (`cursor/*`).
//!
//! Not in the stock ACP schema — typed here so we can answer blocking RPCs
//! (`create_plan`, later `ask_question`) with the nested `outcome` envelope
//! Cursor requires (flat `{ outcome: "accepted" }` is treated as cancel).

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};

/// Blocking: agent wants the user to approve a plan before executing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "cursor/create_plan", response = CursorCreatePlanResponse)]
pub struct CursorCreatePlanRequest {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    /// Markdown body of the plan.
    pub plan: String,
    #[serde(default)]
    pub todos: Vec<CursorPlanTodo>,
    #[serde(default, rename = "isProject")]
    pub is_project: Option<bool>,
    #[serde(default)]
    pub phases: Option<Vec<CursorPlanPhase>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPlanTodo {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub status: CursorPlanTodoStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CursorPlanTodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPlanPhase {
    pub name: String,
    #[serde(default)]
    pub todos: Vec<CursorPlanTodo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
pub struct CursorCreatePlanResponse {
    pub outcome: CursorCreatePlanOutcome,
}

/// Nested discriminant — must serialize as
/// `{ "outcome": { "outcome": "accepted", … } }` (see Cursor ACP docs / HAPI #1044).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CursorCreatePlanOutcome {
    Accepted {
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "planUri")]
        plan_uri: Option<String>,
    },
    Rejected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Cancelled,
}

impl CursorCreatePlanOutcome {
    pub fn accepted() -> Self {
        Self::Accepted { plan_uri: None }
    }

    pub fn cancelled() -> Self {
        Self::Cancelled
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: Some(reason.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_plan_accept_nests_outcome() {
        let raw = serde_json::to_value(CursorCreatePlanResponse {
            outcome: CursorCreatePlanOutcome::accepted(),
        })
        .unwrap();
        assert_eq!(
            raw,
            serde_json::json!({ "outcome": { "outcome": "accepted" } }),
            "flat outcome would be treated as cancel by Cursor"
        );
    }

    #[test]
    fn create_plan_request_roundtrip() {
        use agent_client_protocol::JsonRpcMessage;

        let req = CursorCreatePlanRequest {
            tool_call_id: "call_1".into(),
            name: Some("Tabs".into()),
            overview: Some("Tighten layout".into()),
            plan: "1. Inspect\n2. Fix".into(),
            todos: vec![CursorPlanTodo {
                id: "t1".into(),
                content: "Inspect".into(),
                status: CursorPlanTodoStatus::Pending,
            }],
            is_project: Some(false),
            phases: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["toolCallId"], "call_1");
        assert_eq!(req.method(), "cursor/create_plan");
        let back: CursorCreatePlanRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back.plan, "1. Inspect\n2. Fix");
    }
}
