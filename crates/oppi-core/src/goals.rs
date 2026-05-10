use oppi_protocol::{ThreadGoal, ThreadGoalStatus, ThreadId};
use serde_json::json;

pub const MAX_GOAL_OBJECTIVE_CHARS: usize = 4_000;
const GOAL_CONTINUATION_TEMPLATE: &str =
    include_str!("../../../systemprompts/goals/continuation.md");
const GOAL_BUDGET_LIMIT_TEMPLATE: &str =
    include_str!("../../../systemprompts/goals/budget-limit.md");

pub fn validate_goal_objective(objective: &str) -> Result<String, String> {
    let trimmed = objective.trim();
    if trimmed.is_empty() {
        return Err("goal objective cannot be empty".to_string());
    }
    let chars = trimmed.chars().count();
    if chars > MAX_GOAL_OBJECTIVE_CHARS {
        return Err(format!(
            "goal objective is too long: {chars} characters; limit: {MAX_GOAL_OBJECTIVE_CHARS}. Put longer instructions in a file and reference it from /goal."
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_goal_budget(token_budget: Option<i64>) -> Result<(), String> {
    if token_budget.is_some_and(|budget| budget <= 0) {
        return Err("goal token budget must be a positive integer".to_string());
    }
    Ok(())
}

pub fn new_goal(
    thread_id: ThreadId,
    objective: String,
    status: ThreadGoalStatus,
    token_budget: Option<i64>,
    now_ms: u64,
) -> ThreadGoal {
    ThreadGoal {
        thread_id,
        objective,
        status,
        token_budget,
        tokens_used: 0,
        time_used_seconds: 0,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
    }
}

pub fn status_after_budget(goal: &ThreadGoal) -> ThreadGoalStatus {
    if goal.status == ThreadGoalStatus::Active
        && goal
            .token_budget
            .is_some_and(|budget| goal.tokens_used >= budget)
    {
        ThreadGoalStatus::BudgetLimited
    } else {
        goal.status
    }
}

pub fn apply_goal_accounting_delta(
    goal: &mut ThreadGoal,
    token_delta: i64,
    elapsed_seconds: i64,
    now_ms: u64,
) {
    goal.tokens_used = goal.tokens_used.saturating_add(token_delta.max(0));
    goal.time_used_seconds = goal
        .time_used_seconds
        .saturating_add(elapsed_seconds.max(0));
    goal.status = status_after_budget(goal);
    goal.updated_at_ms = now_ms;
}

pub fn remaining_tokens(goal: &ThreadGoal) -> Option<i64> {
    goal.token_budget.map(|budget| budget - goal.tokens_used)
}

pub fn completion_budget_report(goal: &ThreadGoal) -> Option<String> {
    goal.token_budget.map(|budget| {
        format!(
            "Goal achieved. Report final budget usage to the user: tokens used: {} of {}; time used: {} seconds.",
            goal.tokens_used, budget, goal.time_used_seconds
        )
    })
}

pub fn goal_tool_output(
    goal: Option<&ThreadGoal>,
    completion_budget_report: Option<String>,
) -> String {
    let remaining_tokens = goal.and_then(remaining_tokens);
    serde_json::to_string_pretty(&json!({
        "goal": goal,
        "remainingTokens": remaining_tokens,
        "completionBudgetReport": completion_budget_report,
    }))
    .expect("goal tool payload should serialize")
}

pub fn render_goal_continuation_prompt(goal: &ThreadGoal) -> String {
    render_goal_template(GOAL_CONTINUATION_TEMPLATE, goal)
}

pub fn render_goal_budget_limit_prompt(goal: &ThreadGoal) -> String {
    render_goal_template(GOAL_BUDGET_LIMIT_TEMPLATE, goal)
}

fn render_goal_template(template: &str, goal: &ThreadGoal) -> String {
    template
        .replace("{{ objective }}", &escape_xml_text(&goal.objective))
        .replace(
            "{{ time_used_seconds }}",
            &goal.time_used_seconds.to_string(),
        )
        .replace("{{ tokens_used }}", &goal.tokens_used.to_string())
        .replace(
            "{{ token_budget }}",
            &format_optional_i64(goal.token_budget),
        )
        .replace(
            "{{ remaining_tokens }}",
            &goal
                .token_budget
                .map(|_| format_optional_i64(remaining_tokens(goal)))
                .unwrap_or_else(|| "unbounded".to_string()),
        )
}

fn format_optional_i64(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
