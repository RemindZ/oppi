//! Tool-call bookkeeping for the OPPi runtime spine.

use oppi_protocol::{
    SimulatedToolUse, ToolBatchExecution, ToolCall, ToolCallId, ToolDefinition, ToolExecutionBatch,
    ToolResult,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPairingError {
    DuplicateCall(ToolCallId),
    MissingCall(ToolCallId),
    DuplicateResult(ToolCallId),
    UnresolvedCalls(Vec<ToolCallId>),
}

#[derive(Debug, Default)]
pub struct ToolPairingTracker {
    calls: BTreeMap<ToolCallId, ToolCall>,
    results: BTreeMap<ToolCallId, ToolResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedToolBatch {
    pub execution: ToolBatchExecution,
    pub tools: Vec<SimulatedToolUse>,
}

#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    definitions: BTreeMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, definition: ToolDefinition) -> Option<ToolDefinition> {
        self.definitions.insert(tool_key(&definition), definition)
    }

    pub fn get(&self, namespace: Option<&str>, name: &str) -> Option<&ToolDefinition> {
        self.definitions.get(&tool_key_parts(namespace, name))
    }

    pub fn list(&self) -> Vec<ToolDefinition> {
        self.definitions.values().cloned().collect()
    }
}

fn tool_key(definition: &ToolDefinition) -> String {
    tool_key_parts(definition.namespace.as_deref(), &definition.name)
}

fn tool_key_parts(namespace: Option<&str>, name: &str) -> String {
    format!("{}::{}", namespace.unwrap_or(""), name)
}

pub fn partition_ordered_tool_batches(tools: Vec<SimulatedToolUse>) -> Vec<PlannedToolBatch> {
    let mut batches: Vec<PlannedToolBatch> = Vec::new();
    let mut current_safe: Vec<SimulatedToolUse> = Vec::new();

    for tool in tools {
        if tool.concurrency_safe {
            current_safe.push(tool);
            continue;
        }

        if !current_safe.is_empty() {
            batches.push(PlannedToolBatch {
                execution: ToolBatchExecution::Concurrent,
                tools: std::mem::take(&mut current_safe),
            });
        }
        batches.push(PlannedToolBatch {
            execution: ToolBatchExecution::Exclusive,
            tools: vec![tool],
        });
    }

    if !current_safe.is_empty() {
        batches.push(PlannedToolBatch {
            execution: ToolBatchExecution::Concurrent,
            tools: current_safe,
        });
    }

    batches
}

pub fn describe_tool_batch(
    id: String,
    planned: &PlannedToolBatch,
    concurrency_limit: Option<u32>,
) -> ToolExecutionBatch {
    ToolExecutionBatch {
        id,
        execution: planned.execution,
        tool_call_ids: planned
            .tools
            .iter()
            .map(|tool| tool.call.id.clone())
            .collect(),
        concurrency_limit: if planned.execution == ToolBatchExecution::Concurrent {
            concurrency_limit
        } else {
            None
        },
    }
}

impl ToolPairingTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_call(&mut self, call: ToolCall) -> Result<(), ToolPairingError> {
        if self.calls.contains_key(&call.id) {
            return Err(ToolPairingError::DuplicateCall(call.id));
        }
        self.calls.insert(call.id.clone(), call);
        Ok(())
    }

    pub fn record_result(&mut self, result: ToolResult) -> Result<(), ToolPairingError> {
        if !self.calls.contains_key(&result.call_id) {
            return Err(ToolPairingError::MissingCall(result.call_id));
        }
        if self.results.contains_key(&result.call_id) {
            return Err(ToolPairingError::DuplicateResult(result.call_id));
        }
        self.results.insert(result.call_id.clone(), result);
        Ok(())
    }

    pub fn unresolved_calls(&self) -> Vec<ToolCallId> {
        self.calls
            .keys()
            .filter(|call_id| !self.results.contains_key(*call_id))
            .cloned()
            .collect()
    }

    pub fn finish(&self) -> Result<(), ToolPairingError> {
        let unresolved = self.unresolved_calls();
        if unresolved.is_empty() {
            Ok(())
        } else {
            Err(ToolPairingError::UnresolvedCalls(unresolved))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oppi_protocol::{ToolCall, ToolResultStatus};
    use serde_json::json;

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "read".to_string(),
            namespace: Some("functions".to_string()),
            arguments: json!({ "path": "README.md" }),
        }
    }

    fn simulated(id: &str, concurrency_safe: bool) -> SimulatedToolUse {
        SimulatedToolUse {
            call: call(id),
            result: oppi_protocol::ToolResult {
                call_id: id.to_string(),
                status: ToolResultStatus::Ok,
                output: Some("done".to_string()),
                error: None,
            },
            require_approval: false,
            concurrency_safe,
        }
    }

    #[test]
    fn registry_registers_replaces_and_lists_tools_by_namespace() {
        let mut registry = ToolRegistry::new();
        registry.register(ToolDefinition {
            name: "read".to_string(),
            namespace: Some("functions".to_string()),
            description: Some("Read a file".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: vec!["filesystem".to_string()],
        });
        assert_eq!(
            registry.get(Some("functions"), "read").unwrap().description,
            Some("Read a file".to_string())
        );

        let previous = registry.register(ToolDefinition {
            name: "read".to_string(),
            namespace: Some("functions".to_string()),
            description: Some("Read safely".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: vec!["filesystem".to_string()],
        });
        assert!(previous.is_some());
        assert_eq!(registry.list().len(), 1);
        assert_eq!(
            registry.get(Some("functions"), "read").unwrap().description,
            Some("Read safely".to_string())
        );
    }

    #[test]
    fn partitions_adjacent_safe_tools_without_reordering_unsafe_calls() {
        let batches = partition_ordered_tool_batches(vec![
            simulated("read-1", true),
            simulated("grep-1", true),
            simulated("edit-1", false),
            simulated("read-2", true),
        ]);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].execution, ToolBatchExecution::Concurrent);
        assert_eq!(batches[0].tools.len(), 2);
        assert_eq!(batches[1].execution, ToolBatchExecution::Exclusive);
        assert_eq!(batches[1].tools[0].call.id, "edit-1");
        assert_eq!(batches[2].execution, ToolBatchExecution::Concurrent);
    }

    #[test]
    fn accepts_exactly_one_result_for_each_call() {
        let mut tracker = ToolPairingTracker::new();
        tracker.record_call(call("call-1")).unwrap();
        tracker
            .record_result(ToolResult {
                call_id: "call-1".to_string(),
                status: ToolResultStatus::Ok,
                output: Some("done".to_string()),
                error: None,
            })
            .unwrap();
        assert_eq!(tracker.finish(), Ok(()));
    }

    #[test]
    fn rejects_missing_and_duplicate_results() {
        let mut tracker = ToolPairingTracker::new();
        let missing = tracker.record_result(ToolResult {
            call_id: "missing".to_string(),
            status: ToolResultStatus::Error,
            output: None,
            error: Some("no call".to_string()),
        });
        assert_eq!(
            missing,
            Err(ToolPairingError::MissingCall("missing".to_string()))
        );

        tracker.record_call(call("call-1")).unwrap();
        assert_eq!(
            tracker.finish(),
            Err(ToolPairingError::UnresolvedCalls(vec![
                "call-1".to_string()
            ]))
        );
    }
}
