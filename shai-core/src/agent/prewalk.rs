// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Prewalk: a one-shot handoff of a live session to another model.
//!
//! An agent armed with a prewalk target starts on its usual model and, once
//! the work has visibly moved from planning to editing, continues on the
//! target. "Moved to editing" means both of:
//!
//! * the todo list holds at least one item, and
//! * a `write` or `edit` tool call has completed successfully.
//!
//! The handoff happens between two steps, never in the middle of one: it is
//! checked right before each LLM call, when every result of the previous step
//! is already in the trace. Only the client and model the brain talks to are
//! replaced. The conversation, the todo list, the working directory,
//! permissions and tool state are the same objects before and after.
//!
//! The handoff is one-shot. A [`Prewalk`] carries a latch that is set by the
//! agent that hands off; an agent takes the target out of itself to switch,
//! and no agent armed with the same `Prewalk` switches after that. There is
//! nothing left to switch back with.
//!
//! The agent announces the handoff to whoever watches its events:
//! [`AgentEvent::ModelSwitched`] when it switches, [`AgentEvent::Error`] if
//! its brain turns out to be unable to. Showing that to the user is the job
//! of the front-end. So is saying when a handoff that was asked for is not
//! armed: [`AgentCore::arm_prewalk`] refuses an agent on which it could never
//! happen, instead of leaving it armed for nothing.

use super::{AgentCore, AgentEvent};
use crate::config::roles::BoundModel;
use crate::tools::{ToolResult, BUILTIN_TOOL_GROUP};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

/// Tools whose first successful call marks the move from planning to editing.
pub(crate) const PREWALK_TRIGGER_TOOLS: [&str; 2] = ["write", "edit"];

/// Tool that maintains the todo list.
const TODO_WRITE_TOOL: &str = "todo_write";

/// A handoff that was asked for: where to, and whether it has happened.
///
/// Clones share one latch. A front-end that replaces its agent during a
/// session (another agent, a restored conversation, changed credentials) arms
/// each new agent with a clone, and the first agent to hand off uses the
/// handoff up for all of them. That holds even if the front-end never gets to
/// see the event that announces it.
///
/// Clones also share what the session's tool calls have done towards the
/// trigger: the agents armed with them record their calls in one place. An
/// agent that takes over the same conversation (a swapped agent, `/auth`)
/// hands off as soon as the session is due, without having to plan and edit
/// again. A restored conversation is different: it starts the trigger over
/// with [`Prewalk::reset_signals`], so the saved trace and the conversation
/// just left do not count.
#[derive(Clone)]
pub struct Prewalk {
    target: BoundModel,
    /// One of `PENDING`, `CLAIMED` and `SWITCHED`. It only moves forward.
    state: Arc<AtomicU8>,
    /// What the tool calls of the agents armed with this handoff have done.
    signals: Arc<PrewalkSignals>,
}

/// No agent has handed off yet.
const PENDING: u8 = 0;
/// An agent has taken the handoff and is switching, or failed to.
const CLAIMED: u8 = 1;
/// An agent has switched to the target.
const SWITCHED: u8 = 2;

impl Prewalk {
    /// A handoff to `target` that has not happened yet.
    pub fn new(target: BoundModel) -> Self {
        Self {
            target,
            state: Arc::new(AtomicU8::new(PENDING)),
            signals: Arc::new(PrewalkSignals::default()),
        }
    }

    /// The model the session hands off to.
    pub fn target(&self) -> &BoundModel {
        &self.target
    }

    /// Start the trigger over. The one-shot latch is unchanged, so the
    /// handoff still happens only once in this process.
    ///
    /// A restored conversation calls this so it does not inherit what the
    /// previous conversation, or this conversation's saved trace, already
    /// did. The agent that will run the restored conversation must be armed
    /// after this call, so it holds the new signals.
    pub fn reset_signals(&mut self) {
        self.signals = Arc::new(PrewalkSignals::default());
    }

    /// Whether a non-empty todo list and a successful edit have both been
    /// recorded on these signals.
    pub fn trigger_is_due(&self) -> bool {
        self.signals.handoff_due()
    }

    /// Record a successful built-in tool call, as an armed agent would.
    pub fn record_success(&self, tool_name: &str, todo_count: Option<u64>) {
        let result = if let Some(count) = todo_count {
            ToolResult::success_with_metadata(
                "ok".to_string(),
                [("todo_count".to_string(), serde_json::json!(count))].into(),
            )
        } else {
            ToolResult::success("ok".to_string())
        };
        self.signals.observe(tool_name, &result);
    }

    /// The same one-shot handoff, aimed at `target`: for a target that was
    /// resolved again after the configuration changed.
    pub fn retarget(&self, target: BoundModel) -> Self {
        Self {
            target,
            state: self.state.clone(),
            signals: self.signals.clone(),
        }
    }

    /// True once an agent has handed off, or has tried to: the handoff is
    /// used up either way.
    pub fn is_spent(&self) -> bool {
        self.state.load(Ordering::SeqCst) != PENDING
    }

    /// True once an agent has switched to a target of this handoff.
    pub fn has_switched(&self) -> bool {
        self.state.load(Ordering::SeqCst) == SWITCHED
    }

    /// Take the handoff for the calling agent. False if it is used up.
    fn claim(&self) -> bool {
        self.state
            .compare_exchange(PENDING, CLAIMED, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

impl std::fmt::Debug for Prewalk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Prewalk({}, spent: {})",
            self.target.label(),
            self.is_spent()
        )
    }
}

/// Why an agent cannot be armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PrewalkArmError {
    /// The handoff is used up: an agent of this process has taken it,
    /// whether or not it then managed to switch.
    #[error("the handoff was already tried in this session and is not tried again")]
    Spent,

    /// The agent lacks a tool without which the handoff never comes due.
    #[error("this agent has no built-in {missing} tool, so the handoff could never happen")]
    NoTrigger { missing: &'static str },

    /// The agent cannot change the model it runs on.
    #[error("this agent cannot change model")]
    CannotSwitch,
}

/// What the session's tool calls have done so far, as far as prewalk cares.
///
/// The signals are recorded where each tool call completes, from the call's
/// own result. They do not depend on which todo storage or operation log the
/// tools were built with, which differs between the ways an agent is set up.
#[derive(Debug, Default)]
pub(crate) struct PrewalkSignals {
    /// A `write` or `edit` call has succeeded at least once.
    edited: AtomicBool,
    /// Number of items the last successful `todo_write` left in the list.
    todo_items: AtomicUsize,
}

impl PrewalkSignals {
    /// Record the outcome of one completed call to a built-in tool.
    pub(crate) fn observe(&self, tool_name: &str, result: &ToolResult) {
        if !result.is_success() {
            return;
        }
        if PREWALK_TRIGGER_TOOLS.contains(&tool_name) {
            self.edited.store(true, Ordering::SeqCst);
        }
        if tool_name == TODO_WRITE_TOOL {
            // The tool reports how many items it stored. Without that figure
            // the list counts as empty, which keeps the handoff from firing.
            // Two `todo_write` calls in one step complete in no fixed order,
            // here as in the todo storage itself: the last one recorded wins.
            let items = result
                .metadata()
                .and_then(|metadata| metadata.get("todo_count"))
                .and_then(|count| count.as_u64())
                .unwrap_or(0);
            self.todo_items.store(items as usize, Ordering::SeqCst);
        }
    }

    /// True once the todo list is not empty and an edit has succeeded.
    pub(crate) fn handoff_due(&self) -> bool {
        self.edited.load(Ordering::SeqCst) && self.todo_items.load(Ordering::SeqCst) > 0
    }
}

impl AgentCore {
    /// Arm a one-shot handoff, if it can happen on this agent.
    ///
    /// It cannot when the handoff is already used up, when the agent has no
    /// built-in `todo_write` or neither `write` nor `edit` (the handoff would
    /// never come due), or when its brain cannot change model. The agent is
    /// then left as it was, and the caller is expected to tell the user:
    /// an agent that reads as armed and can never hand off would be a
    /// handoff silently dropped.
    pub fn arm_prewalk(&mut self, prewalk: Prewalk) -> Result<(), PrewalkArmError> {
        if prewalk.is_spent() {
            return Err(PrewalkArmError::Spent);
        }
        // The tool a call by that name is dispatched to is the first one
        // listed under it: that one has to be the built-in.
        let has_builtin = |name: &str| {
            self.tool_ctx
                .available_tools
                .iter()
                .find(|tool| tool.name() == name)
                .is_some_and(|tool| tool.group() == Some(BUILTIN_TOOL_GROUP))
        };
        if !has_builtin(TODO_WRITE_TOOL) {
            return Err(PrewalkArmError::NoTrigger {
                missing: TODO_WRITE_TOOL,
            });
        }
        if !PREWALK_TRIGGER_TOOLS.iter().any(|name| has_builtin(name)) {
            return Err(PrewalkArmError::NoTrigger {
                missing: "write or edit",
            });
        }
        // Nothing else holds the brain before the agent runs. If something
        // does, the question cannot be answered, and the answer is no.
        let can_switch = self
            .brain
            .try_read()
            .map(|brain| brain.can_switch_model())
            .unwrap_or(false);
        if !can_switch {
            return Err(PrewalkArmError::CannotSwitch);
        }
        // From now on this agent's calls count towards the session's trigger.
        self.tool_ctx.prewalk_signals = prewalk.signals.clone();
        self.prewalk = Some(prewalk);
        Ok(())
    }

    /// True while a handoff is armed and has not happened yet.
    pub fn is_prewalk_armed(&self) -> bool {
        self.prewalk
            .as_ref()
            .is_some_and(|prewalk| !prewalk.is_spent())
    }

    /// Hand the session off to the prewalk target if it is armed and due.
    ///
    /// Called on the agent loop right before each LLM call. It never waits:
    /// if the brain is busy, which the state machine does not rule out when a
    /// late result of a cancelled step is still being handled, the agent
    /// stays armed and the handoff is tried again before the next call.
    pub(crate) async fn prewalk_if_due(&mut self) {
        let Some(prewalk) = self.prewalk.as_ref() else {
            return;
        };
        if prewalk.is_spent() {
            // Another agent of this session has handed off already.
            self.prewalk = None;
            return;
        }
        if !self.tool_ctx.prewalk_signals.handoff_due() {
            return;
        }
        let Ok(mut brain) = self.brain.try_write() else {
            return;
        };
        // Disarm first: whatever happens below, this never runs again, here
        // or in any other agent armed with the same handoff.
        let Some(prewalk) = self.prewalk.take() else {
            return;
        };
        if !prewalk.claim() {
            return;
        }
        let target = &prewalk.target;
        let switched = brain.switch_model(target.llm().clone(), target.model().to_string());
        drop(brain);

        if switched {
            prewalk.state.store(SWITCHED, Ordering::SeqCst);
            info!(target: "agent::prewalk", to = %target.label(), "session handed off");
            let _ = self
                .emit_event(AgentEvent::ModelSwitched {
                    provider: target.provider().to_string(),
                    model: target.model().to_string(),
                })
                .await;
        } else {
            let _ = self
                .emit_event(AgentEvent::Error {
                    error: format!(
                        "prewalk: this agent cannot change model, staying on the current one instead of switching to {}",
                        target.label()
                    ),
                })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::brain::{ThinkerContext, ThinkerDecision, ToolBudgetRef};
    use crate::agent::scripted_llm::{call, calls, text, Script, Served};
    use crate::agent::{Agent, AgentBuilder, AgentError, Brain};
    use crate::runners::coder::CoderBrain;
    use crate::tools::mcp::{get_mcp_tools, McpClient, McpToolDescription};
    use crate::tools::{AnyTool, FsOperationLog, TodoStorage, ToolCall};
    use async_trait::async_trait;
    use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
    use serde_json::{json, Value};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;

    const START: &str = "start";
    const TARGET: &str = "target";
    const START_MODEL: &str = "start-model";
    const TARGET_MODEL: &str = "target-model";

    struct Outcome {
        script: Script,
        events: Vec<AgentEvent>,
        todos: Arc<TodoStorage>,
        trace: Vec<ChatMessage>,
    }

    impl Outcome {
        fn served(&self) -> Vec<Served> {
            self.script.served()
        }

        fn switches(&self) -> Vec<(String, String)> {
            self.events
                .iter()
                .filter_map(|event| match event {
                    AgentEvent::ModelSwitched { provider, model } => {
                        Some((provider.clone(), model.clone()))
                    }
                    _ => None,
                })
                .collect()
        }

        /// How many LLM replies the agent had processed when it switched.
        fn replies_before_switch(&self) -> Option<usize> {
            let at = self
                .events
                .iter()
                .position(|event| matches!(event, AgentEvent::ModelSwitched { .. }))?;
            Some(
                self.events[..at]
                    .iter()
                    .filter(|event| matches!(event, AgentEvent::BrainResult { .. }))
                    .count(),
            )
        }
    }

    fn todo_write() -> ChatMessage {
        call(
            "todo_write",
            json!({"todos": [
                {"content": "write the file", "status": "in_progress"},
                {"content": "check it", "status": "pending"}
            ]}),
        )
    }

    fn write_args(path: &Path, content: &str) -> Value {
        json!({"files": [{"path": path.to_string_lossy(), "content": content}]})
    }

    fn write(path: &Path, content: &str) -> ChatMessage {
        call("write", write_args(path, content))
    }

    fn ls(path: &Path) -> ChatMessage {
        call("ls", json!({"directory": path.to_string_lossy()}))
    }

    /// A handoff to the scripted target that has not happened yet.
    fn target(script: &Script) -> Prewalk {
        Prewalk::new(BoundModel::new(script.client(TARGET), TARGET_MODEL))
    }

    fn default_tools() -> Vec<Box<dyn AnyTool>> {
        AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new())).0
    }

    fn default_tools_without(names: &[&str]) -> Vec<Box<dyn AnyTool>> {
        default_tools()
            .into_iter()
            .filter(|tool| !names.contains(&tool.name().as_str()))
            .collect()
    }

    /// An agent on the real coder brain, served by `START`, not armed.
    fn coder_agent(script: &Script, tools: Vec<Box<dyn AnyTool>>) -> AgentCore {
        let brain = CoderBrain::new(script.client(START), START_MODEL.to_string());
        AgentBuilder::with_brain(Box::new(brain))
            .tools(tools)
            .goal("create the file")
            .sudo()
            .build()
    }

    /// A `todo_write` result that reports `count` items.
    fn todo_result(count: u64) -> ToolResult {
        ToolResult::success_with_metadata(
            "ok".to_string(),
            [("todo_count".to_string(), json!(count))].into(),
        )
    }

    /// Make the handoff of `agent` due without running it.
    fn make_due(agent: &AgentCore) {
        let signals = &agent.tool_ctx.prewalk_signals;
        signals.observe("write", &ToolResult::success("ok".to_string()));
        signals.observe("todo_write", &todo_result(1));
        assert!(signals.handoff_due());
    }

    /// An MCP server that offers one tool, which always succeeds.
    struct OneToolServer {
        tool: &'static str,
    }

    #[async_trait]
    impl McpClient for OneToolServer {
        async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn list_tools(
            &self,
        ) -> Result<Vec<McpToolDescription>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![McpToolDescription {
                name: self.tool.to_string(),
                description: "a tool served from elsewhere".to_string(),
                parameters_schema: json!({"type": "object", "properties": {}}),
            }])
        }

        async fn execute_tool(
            &self,
            _: ToolCall,
        ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
            Ok(ToolResult::success("done by the server".to_string()))
        }
    }

    /// The tools of an MCP server configured under `server_name`.
    async fn server_tools(server_name: &str, tool: &'static str) -> Vec<Box<dyn AnyTool>> {
        get_mcp_tools(Box::new(OneToolServer { tool }), server_name)
            .await
            .expect("the scripted server lists its tools")
    }

    /// Wait until the scripted LLM has received `count` requests.
    async fn served_reaches(script: &Script, count: usize) {
        tokio::time::timeout(Duration::from_secs(60), async {
            while script.served().len() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the agent should have asked the model again");
    }

    /// Run the real agent loop, with the real coder brain and built-in tools,
    /// against a scripted LLM. `START` serves the session until a handoff.
    async fn run(replies: Vec<ChatMessage>, armed: bool) -> Outcome {
        run_with(replies, armed, true).await
    }

    async fn run_with(replies: Vec<ChatMessage>, armed: bool, sudo: bool) -> Outcome {
        let script = Script::new(replies);
        let (tools, todos) = AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new()));
        let brain = CoderBrain::new(script.client(START), START_MODEL.to_string());

        let mut builder = AgentBuilder::with_brain(Box::new(brain))
            .tools(tools)
            .set_todo_storage(todos.clone())
            .goal("create the file");
        if sudo {
            builder = builder.sudo();
        }
        let mut agent = builder.build();
        if armed {
            agent
                .arm_prewalk(target(&script))
                .expect("the default toolbox and the coder brain can be armed");
        }

        // Without sudo, a subscriber would make the agent wait for a permission
        // answer; with none, the write is denied outright.
        let mut events_rx = sudo.then(|| agent.watch());
        let result = tokio::time::timeout(Duration::from_secs(60), agent.run())
            .await
            .expect("the agent run should finish")
            .expect("the agent run should succeed");

        let mut events = Vec::new();
        if let Some(rx) = events_rx.as_mut() {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        Outcome {
            script,
            events,
            todos,
            trace: result.trace,
        }
    }

    #[tokio::test]
    async fn switches_once_at_the_boundary_after_the_first_edit() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let outcome = run(
            vec![
                todo_write(),
                write(&file, "hello"),
                ls(dir.path()),
                text("done"),
            ],
            true,
        )
        .await;

        // The step that planned and the step that wrote ran on the starting
        // model; every later step ran on the target.
        assert_eq!(outcome.script.served_by(), [START, START, TARGET, TARGET]);
        assert_eq!(outcome.script.remaining(), 0);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");

        // Exactly one switch, announced, and placed after the reply that
        // carried the write and before the next reply.
        assert_eq!(
            outcome.switches(),
            [(TARGET.to_string(), TARGET_MODEL.to_string())]
        );
        assert_eq!(outcome.replies_before_switch(), Some(2));

        let served = outcome.served();
        assert_eq!(served[0].model, START_MODEL);
        assert_eq!(served[1].model, START_MODEL);
        assert_eq!(served[2].model, TARGET_MODEL);
        assert_eq!(served[3].model, TARGET_MODEL);
    }

    #[tokio::test]
    async fn conversation_todos_and_tools_carry_over_unchanged() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let outcome = run(
            vec![todo_write(), write(&file, "hello"), text("done")],
            true,
        )
        .await;
        assert_eq!(outcome.script.served_by(), [START, START, TARGET]);

        let served = outcome.served();
        let (before, after) = (&served[1], &served[2]);

        // The target sees the whole conversation so far: everything the
        // starting model was sent for the previous step, then that step's
        // reply and its tool result. Index 0 is the per-step system prompt.
        assert_eq!(
            after.messages[1..before.messages.len()],
            before.messages[1..]
        );
        let added = &after.messages[before.messages.len()..];
        assert_eq!(added.len(), 2, "{added:?}");
        assert!(matches!(added[0], ChatMessage::Assistant { .. }));
        assert!(matches!(added[1], ChatMessage::Tool { .. }));
        assert!(matches!(
            &after.messages[1],
            ChatMessage::User { content: ChatMessageContent::Text(goal), .. } if goal == "create the file"
        ));

        // Same tools on offer, same todo list, and nothing extra in the trace.
        assert_eq!(after.tools, before.tools);
        let todos = outcome.todos.get_all().await;
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "write the file");
        assert!(!outcome
            .trace
            .iter()
            .any(|message| matches!(message, ChatMessage::System { .. })));
        assert_eq!(outcome.trace.len(), after.messages.len());
    }

    #[tokio::test]
    async fn never_switches_before_the_first_edit() {
        let dir = TempDir::new().unwrap();
        let outcome = run(
            vec![
                todo_write(),
                ls(dir.path()),
                ls(dir.path()),
                text("nothing to change"),
            ],
            true,
        )
        .await;

        assert_eq!(outcome.script.served_by(), [START; 4]);
        assert!(outcome.switches().is_empty());
    }

    #[tokio::test]
    async fn an_edit_without_a_todo_list_does_not_switch() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let outcome = run(
            vec![write(&file, "hello"), ls(dir.path()), text("done")],
            true,
        )
        .await;

        assert_eq!(outcome.script.served_by(), [START; 3]);
        assert!(outcome.switches().is_empty());
    }

    #[tokio::test]
    async fn switches_when_the_todo_list_arrives_after_the_edit() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let outcome = run(
            vec![
                write(&file, "hello"),
                ls(dir.path()),
                todo_write(),
                text("done"),
            ],
            true,
        )
        .await;

        // Both conditions hold only once the list exists.
        assert_eq!(outcome.script.served_by(), [START, START, START, TARGET]);
        assert_eq!(outcome.replies_before_switch(), Some(3));
        assert_eq!(outcome.switches().len(), 1);
    }

    #[tokio::test]
    async fn todo_list_and_edit_in_the_same_step_switch_after_that_step() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let plan_and_write = calls(&[
            (
                "todo_write",
                json!({"todos": [{"content": "write the file", "status": "in_progress"}]}),
            ),
            ("write", write_args(&file, "hello")),
        ]);
        let outcome = run(vec![plan_and_write, text("done")], true).await;

        assert_eq!(outcome.script.served_by(), [START, TARGET]);
        assert_eq!(outcome.replies_before_switch(), Some(1));
    }

    #[tokio::test]
    async fn a_failed_edit_does_not_switch() {
        let dir = TempDir::new().unwrap();
        // A regular file where the write needs a directory.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let outcome = run(
            vec![
                todo_write(),
                write(&blocker.join("out.txt"), "hello"),
                ls(dir.path()),
                text("giving up"),
            ],
            true,
        )
        .await;

        assert_eq!(outcome.script.served_by(), [START; 4]);
        assert!(outcome.switches().is_empty());
    }

    #[tokio::test]
    async fn a_denied_edit_does_not_switch() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let script = Script::new(vec![
            todo_write(),
            write(&file, "hello"),
            text("unreachable"),
        ]);
        let (tools, todos) = AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new()));
        let brain = CoderBrain::new(script.client(START), START_MODEL.to_string());
        // No sudo and nobody to ask: the write is denied and the run stops.
        let mut agent = AgentBuilder::with_brain(Box::new(brain))
            .tools(tools)
            .set_todo_storage(todos.clone())
            .goal("create the file")
            .build();
        let prewalk = target(&script);
        agent.arm_prewalk(prewalk.clone()).unwrap();

        tokio::time::timeout(Duration::from_secs(60), agent.run())
            .await
            .expect("the agent run should finish")
            .expect("the agent run should succeed");

        assert_eq!(script.served_by(), [START, START]);
        assert!(!file.exists());
        assert_eq!(todos.get_all().await.len(), 2);

        // The todo list is there, so an edit that counted would have made
        // the handoff due. The denied one left nothing behind: the agent is
        // still armed, and nothing happens at the next boundary either.
        assert!(!agent.tool_ctx.prewalk_signals.handoff_due());
        agent.prewalk_if_due().await;
        assert!(agent.is_prewalk_armed());
        assert!(!prewalk.is_spent());
    }

    #[tokio::test]
    async fn switches_exactly_once_and_never_back() {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        let outcome = run(
            vec![
                todo_write(),
                write(&first, "one"),
                write(&second, "two"),
                todo_write(),
                write(&first, "three"),
                text("done"),
            ],
            true,
        )
        .await;

        assert_eq!(
            outcome.script.served_by(),
            [START, START, TARGET, TARGET, TARGET, TARGET]
        );
        assert_eq!(outcome.switches().len(), 1);
        assert_eq!(outcome.replies_before_switch(), Some(2));
    }

    #[tokio::test]
    async fn an_unarmed_session_never_switches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let outcome = run(
            vec![todo_write(), write(&file, "hello"), text("done")],
            false,
        )
        .await;

        assert_eq!(outcome.script.served_by(), [START; 3]);
        assert!(outcome.switches().is_empty());
    }

    /// A brain that scripts its own replies and keeps the default
    /// `switch_model`, which refuses.
    struct FixedBrain {
        replies: std::collections::VecDeque<ChatMessage>,
        /// What it answers when asked whether it can switch.
        claims_it_can_switch: bool,
    }

    #[async_trait]
    impl Brain for FixedBrain {
        async fn next_step(
            &mut self,
            _: ThinkerContext,
            _: ToolBudgetRef,
        ) -> Result<ThinkerDecision, AgentError> {
            let reply = self.replies.pop_front().unwrap_or_else(|| text("done"));
            Ok(ThinkerDecision::agent_pause(reply))
        }

        fn can_switch_model(&self) -> bool {
            self.claims_it_can_switch
        }
    }

    #[tokio::test]
    async fn a_brain_that_fails_to_switch_says_so_and_keeps_running() {
        // Arming already refuses a brain that cannot switch. This is the
        // second line: one that said it could and then does not.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let script = Script::new(vec![]);
        let (tools, todos) = AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new()));
        let brain = FixedBrain {
            replies: vec![
                todo_write(),
                write(&file, "hello"),
                ls(dir.path()),
                text("done"),
            ]
            .into(),
            claims_it_can_switch: true,
        };
        let mut agent = AgentBuilder::with_brain(Box::new(brain))
            .tools(tools)
            .set_todo_storage(todos)
            .goal("create the file")
            .sudo()
            .build();
        let prewalk = target(&script);
        agent.arm_prewalk(prewalk.clone()).unwrap();
        assert!(agent.is_prewalk_armed());

        let mut events_rx = agent.watch();
        tokio::time::timeout(Duration::from_secs(60), agent.run())
            .await
            .expect("the agent run should finish")
            .expect("the agent run should succeed");
        assert!(!agent.is_prewalk_armed());

        let mut errors = Vec::new();
        let mut switches = 0;
        while let Ok(event) = events_rx.try_recv() {
            match event {
                AgentEvent::Error { error } => errors.push(error),
                AgentEvent::ModelSwitched { .. } => switches += 1,
                _ => {}
            }
        }
        // Reported once, not silently dropped, and the target is never used.
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("prewalk"), "{errors:?}");
        assert!(errors[0].contains("target/target-model"), "{errors:?}");
        assert_eq!(switches, 0);
        assert!(script.served().is_empty());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
        // The handoff is used up all the same: it is not tried again. It is
        // not reported as a switch either.
        assert!(prewalk.is_spent());
        assert!(!prewalk.has_switched());
    }

    #[tokio::test]
    async fn arming_is_refused_when_the_handoff_could_never_happen() {
        let script = Script::new(vec![]);

        // The usual toolbox on the coder brain: armed.
        let mut agent = coder_agent(&script, default_tools());
        assert!(!agent.is_prewalk_armed());
        assert_eq!(agent.arm_prewalk(target(&script)), Ok(()));
        assert!(agent.is_prewalk_armed());

        // One of `write` and `edit` is enough.
        for removed in ["write", "edit"] {
            let mut agent = coder_agent(&script, default_tools_without(&[removed]));
            assert_eq!(agent.arm_prewalk(target(&script)), Ok(()), "{removed}");
        }

        // Without a todo list, or without anything to edit with, the handoff
        // would never come due.
        for (removed, missing) in [
            (&["todo_write"][..], "todo_write"),
            (&["write", "edit"][..], "write or edit"),
        ] {
            let mut agent = coder_agent(&script, default_tools_without(removed));
            let error = agent.arm_prewalk(target(&script)).unwrap_err();
            assert_eq!(error, PrewalkArmError::NoTrigger { missing });
            assert!(error.to_string().contains(missing), "{error}");
            assert!(!agent.is_prewalk_armed());
        }

        // Tools of the same names served by an MCP server do not count,
        // whatever the server is called.
        for server in ["files", BUILTIN_TOOL_GROUP] {
            let mut tools = default_tools_without(&["write", "edit"]);
            tools.extend(server_tools(server, "write").await);
            tools.extend(server_tools(server, "edit").await);
            let mut agent = coder_agent(&script, tools);
            assert_eq!(
                agent.arm_prewalk(target(&script)),
                Err(PrewalkArmError::NoTrigger {
                    missing: "write or edit"
                }),
                "{server}"
            );
        }

        // Built-in tools that a server's tools of the same names come
        // before: the calls go to the server's tools.
        let mut tools = server_tools("files", "write").await;
        tools.extend(server_tools("files", "edit").await);
        tools.extend(default_tools());
        let mut agent = coder_agent(&script, tools);
        assert_eq!(
            agent.arm_prewalk(target(&script)),
            Err(PrewalkArmError::NoTrigger {
                missing: "write or edit"
            })
        );

        // A brain that cannot change model.
        let brain = FixedBrain {
            replies: Default::default(),
            claims_it_can_switch: false,
        };
        let mut agent = AgentBuilder::with_brain(Box::new(brain))
            .tools(default_tools())
            .build();
        assert_eq!(
            agent.arm_prewalk(target(&script)),
            Err(PrewalkArmError::CannotSwitch)
        );
        assert!(!agent.is_prewalk_armed());

        // A handoff that is used up.
        let spent = target(&script);
        assert!(spent.claim());
        assert!(!spent.claim());
        let mut agent = coder_agent(&script, default_tools());
        assert_eq!(agent.arm_prewalk(spent), Err(PrewalkArmError::Spent));
        assert!(!agent.is_prewalk_armed());

        assert!(script.served().is_empty());
    }

    #[tokio::test]
    async fn a_server_tool_named_like_an_edit_tool_does_not_trigger_the_handoff() {
        // The agent keeps the built-in `edit`, so it can be armed, but `write`
        // comes from an MCP server that the config file calls "builtin".
        let dir = TempDir::new().unwrap();
        let script = Script::new(vec![
            todo_write(),
            call("write", json!({})),
            ls(dir.path()),
            text("done"),
        ]);
        let mut tools = default_tools_without(&["write"]);
        let from_server = server_tools(BUILTIN_TOOL_GROUP, "write").await;
        assert!(from_server
            .iter()
            .all(|tool| tool.group() != Some(BUILTIN_TOOL_GROUP)));
        tools.extend(from_server);
        let mut agent = coder_agent(&script, tools);
        agent.arm_prewalk(target(&script)).unwrap();

        let result = tokio::time::timeout(Duration::from_secs(60), agent.run())
            .await
            .expect("the agent run should finish")
            .expect("the agent run should succeed");

        // The server's tool ran and succeeded...
        assert!(result.trace.iter().any(|message| matches!(
            message,
            ChatMessage::Tool { content: ChatMessageContent::Text(text), .. }
                if text.contains("done by the server")
        )));
        // ...and the session stayed where it was.
        assert_eq!(script.served_by(), [START; 4]);
        assert!(agent.is_prewalk_armed());
    }

    #[tokio::test]
    async fn a_busy_brain_defers_the_handoff_instead_of_blocking_the_agent() {
        let script = Script::new(vec![]);
        let mut agent = coder_agent(&script, default_tools());
        let prewalk = target(&script);
        agent.arm_prewalk(prewalk.clone()).unwrap();
        let mut events = agent.watch();
        make_due(&agent);

        // Something still holds the brain, as a request in flight would.
        let brain = agent.brain.clone();
        let busy = brain.write().await;
        tokio::time::timeout(Duration::from_secs(10), agent.prewalk_if_due())
            .await
            .expect("the agent loop must not wait for the brain");
        assert!(agent.is_prewalk_armed());
        assert!(!prewalk.is_spent());
        assert!(events.try_recv().is_err());
        drop(busy);

        // At the next boundary the brain is free, and the handoff happens.
        agent.prewalk_if_due().await;
        assert!(!agent.is_prewalk_armed());
        assert!(prewalk.has_switched());
        assert!(matches!(
            events.try_recv(),
            Ok(AgentEvent::ModelSwitched { .. })
        ));

        // Once.
        agent.prewalk_if_due().await;
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn agents_armed_with_the_same_handoff_switch_once_between_them() {
        let script = Script::new(vec![]);
        let prewalk = target(&script);
        let mut first = coder_agent(&script, default_tools());
        let mut second = coder_agent(&script, default_tools());
        first.arm_prewalk(prewalk.clone()).unwrap();
        // Aimed again, as after a configuration change: the same one-shot.
        let again = prewalk.retarget(BoundModel::new(script.client("other"), "other-model"));
        assert_eq!(again.target().label(), "other/other-model");
        second.arm_prewalk(again).unwrap();
        let mut first_events = first.watch();
        let mut second_events = second.watch();
        make_due(&first);
        make_due(&second);

        first.prewalk_if_due().await;
        assert!(prewalk.is_spent());
        assert!(matches!(
            first_events.try_recv(),
            Ok(AgentEvent::ModelSwitched { .. })
        ));

        // The second agent was armed before the first one switched. It does
        // not switch after it, and no agent can be armed any more.
        assert!(!second.is_prewalk_armed());
        second.prewalk_if_due().await;
        assert!(second_events.try_recv().is_err());
        let mut third = coder_agent(&script, default_tools());
        assert_eq!(
            third.arm_prewalk(prewalk.clone()),
            Err(PrewalkArmError::Spent)
        );
        assert!(script.served().is_empty());
    }

    #[tokio::test]
    async fn what_the_session_did_counts_for_the_agent_that_takes_over() {
        let script = Script::new(vec![]);
        let prewalk = target(&script);
        let mut first = coder_agent(&script, default_tools());
        first.arm_prewalk(prewalk.clone()).unwrap();
        // The first agent edits, and is replaced before its next request.
        first
            .tool_ctx
            .prewalk_signals
            .observe("write", &ToolResult::success("ok".to_string()));

        // The agent that takes over the conversation does not edit again.
        let mut second = coder_agent(&script, default_tools());
        second.arm_prewalk(prewalk.clone()).unwrap();
        let mut events = second.watch();
        second.prewalk_if_due().await;
        assert!(!prewalk.is_spent(), "no todo list yet");
        second
            .tool_ctx
            .prewalk_signals
            .observe("todo_write", &todo_result(1));
        second.prewalk_if_due().await;
        assert!(prewalk.has_switched());
        assert!(matches!(
            events.try_recv(),
            Ok(AgentEvent::ModelSwitched { .. })
        ));

        // A session that planned and edited before its agent was replaced
        // hands off at the first boundary of the next agent.
        let prewalk = target(&script);
        let mut first = coder_agent(&script, default_tools());
        first.arm_prewalk(prewalk.clone()).unwrap();
        make_due(&first);
        let mut next = coder_agent(&script, default_tools());
        next.arm_prewalk(prewalk.clone()).unwrap();
        next.prewalk_if_due().await;
        assert!(prewalk.has_switched());
        assert!(script.served().is_empty());
    }

    #[tokio::test]
    async fn later_turns_and_a_regenerate_stay_on_the_target() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let script = Script::new(vec![
            todo_write(),
            write(&file, "hello"),
            text("done"),
            text("second answer"),
            text("second answer, again"),
        ]);
        let mut agent = coder_agent(&script, default_tools());
        agent.arm_prewalk(target(&script)).unwrap();
        let controller = agent.controller();
        let mut events = agent.watch();
        let run = tokio::spawn(async move { agent.run().await });

        // First turn: planned and wrote on the starting model, finished on
        // the target.
        served_reaches(&script, 3).await;
        controller.wait_turn(Some(60_000)).await.unwrap();
        assert_eq!(script.served_by(), [START, START, TARGET]);

        // A new request from the user, then the same answer asked for again:
        // both are served by the target.
        controller
            .send_user_input("and now?".to_string())
            .await
            .unwrap();
        served_reaches(&script, 4).await;
        controller.wait_turn(Some(60_000)).await.unwrap();
        controller.regenerate().await.unwrap();
        served_reaches(&script, 5).await;
        controller.wait_turn(Some(60_000)).await.unwrap();

        assert_eq!(script.served_by(), [START, START, TARGET, TARGET, TARGET]);
        let served = script.served();
        assert!(served[3..].iter().all(|s| s.model == TARGET_MODEL));

        controller.terminate().await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(60), run).await;
        let mut switches = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(event, AgentEvent::ModelSwitched { .. }) {
                switches += 1;
            }
        }
        assert_eq!(switches, 1);
    }

    #[tokio::test]
    async fn switches_even_when_the_agent_does_not_hold_the_tools_todo_storage() {
        // Some front-ends build the tools with one todo storage and the agent
        // with another. The handoff must not depend on which one the agent holds.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let script = Script::new(vec![todo_write(), write(&file, "hello"), text("done")]);
        let (tools, _tools_todos) =
            AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new()));
        let mut agent = coder_agent(&script, tools);
        agent.arm_prewalk(target(&script)).unwrap();

        tokio::time::timeout(Duration::from_secs(60), agent.run())
            .await
            .expect("the agent run should finish")
            .expect("the agent run should succeed");

        assert!(agent.tool_ctx.todo_storage.get_all().await.is_empty());
        assert_eq!(script.served_by(), [START, START, TARGET]);
    }

    #[tokio::test]
    async fn clearing_the_todo_list_before_the_edit_keeps_the_session_where_it_is() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let outcome = run(
            vec![
                todo_write(),
                call("todo_write", json!({"todos": []})),
                write(&file, "hello"),
                text("done"),
            ],
            true,
        )
        .await;

        assert_eq!(outcome.script.served_by(), [START; 4]);
        assert!(outcome.switches().is_empty());
    }

    #[test]
    fn signals_only_count_successful_calls() {
        let signals = PrewalkSignals::default();
        let todo = todo_result;

        // Failures and denials leave no trace.
        signals.observe("write", &ToolResult::error("disk full".to_string()));
        signals.observe("edit", &ToolResult::denied());
        signals.observe("todo_write", &ToolResult::error("bad".to_string()));
        assert!(!signals.handoff_due());

        // Other tools never count as an edit.
        for tool in ["bash", "read", "fetch", "memory_write", "todo_read"] {
            signals.observe(tool, &ToolResult::success("ok".to_string()));
        }
        signals.observe("todo_write", &todo(2));
        assert!(!signals.handoff_due());

        signals.observe("edit", &ToolResult::success("ok".to_string()));
        assert!(signals.handoff_due());

        // An emptied list, or a result that does not say how many items it
        // holds, counts as no list.
        signals.observe("todo_write", &todo(0));
        assert!(!signals.handoff_due());
        signals.observe("todo_write", &todo(1));
        assert!(signals.handoff_due());
        signals.observe("todo_write", &ToolResult::success("ok".to_string()));
        assert!(!signals.handoff_due());
    }

    #[test]
    fn reset_signals_starts_the_trigger_over_and_keeps_the_latch() {
        let target = BoundModel::new(
            std::sync::Arc::new(shai_llm::LlmClient::ollama(
                "http://127.0.0.1:9/v1".to_string(),
            )),
            "target",
        );
        let mut prewalk = Prewalk::new(target);
        prewalk.record_success("write", None);
        prewalk.record_success("todo_write", Some(1));
        assert!(prewalk.trigger_is_due());
        assert!(!prewalk.is_spent());

        let before = prewalk.clone();
        prewalk.reset_signals();
        assert!(!prewalk.trigger_is_due());
        assert!(!prewalk.is_spent());
        // The previous conversation's copy still has the old signals.
        // The latch is shared: spending one spends the other.
        assert!(before.trigger_is_due());
        assert!(!before.is_spent());
    }
}
