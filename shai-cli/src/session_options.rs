// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Options of an agent session that are settled when it starts.
//!
//! They come from the command line and from the auth config, and are resolved
//! before any model is contacted. An option that was asked for and cannot be
//! resolved stops the session from starting: it must not start as if the
//! option had not been given. So does a config file that exists and cannot be
//! read, since it may ask for any of them.
//!
//! Resolving an option is not yet honouring it. Whether it applies to the
//! agent a session runs is decided when that agent is armed, and the user is
//! told when it does not.

use shai_core::agent::output::{
    prewalk_armed_notice, prewalk_disarmed_notice, prewalk_not_armed_notice, prewalk_over_notice,
};
use shai_core::agent::{AgentCore, AgentEvent, Prewalk, PrewalkArmError};
use shai_core::config::config::ShaiConfig;
use shai_core::config::prewalk::resolve_prewalk;
use shai_core::config::roles::RoleError;

/// What a session was asked to do beyond running on the primary model.
#[derive(Clone, Default)]
pub struct SessionOptions {
    /// Value of `--prewalk-into`, kept to resolve the target again when the
    /// config changes during the session.
    prewalk_into: Option<String>,
    /// The handoff this session was asked for, until it is over.
    prewalk: Option<Prewalk>,
    /// The target the user was last told is armed.
    announced: Option<String>,
    /// Where the session went, once the handoff has happened.
    handed_off: Option<String>,
}

impl SessionOptions {
    /// Resolve the options of a session about to start.
    ///
    /// `prewalk_into` is the value of `--prewalk-into`. Nothing here performs
    /// network I/O.
    pub fn resolve(prewalk_into: Option<&str>) -> Result<Self, String> {
        // The user's own config, never the built-in default.
        let config = ShaiConfig::load_for_roles();
        Self::resolve_from(prewalk_into, config.as_ref())
    }

    fn resolve_from(
        prewalk_into: Option<&str>,
        config: Result<&ShaiConfig, &RoleError>,
    ) -> Result<Self, String> {
        if let Some(problem) = Self::unreadable(config) {
            return Err(format!(
                "{problem}. A session does not start on a config file it cannot read, \
                 since the file may ask for options the session would run without: \
                 fix the file, or move it away"
            ));
        }
        let prewalk = Self::resolve_prewalk(prewalk_into, config)?;
        Ok(Self {
            prewalk_into: prewalk_into.map(str::to_string),
            prewalk: prewalk.map(Prewalk::new),
            announced: None,
            handed_off: None,
        })
    }

    /// A missing config file asks for nothing. One that is there and cannot
    /// be read may ask for anything: running without what it asks for would
    /// be a guess. Returns what to say about such a file.
    fn unreadable(config: Result<&ShaiConfig, &RoleError>) -> Option<String> {
        match config {
            Err(RoleError::ConfigInvalid { path, reason }) => Some(format!(
                "the config file {path} could not be loaded ({reason})"
            )),
            _ => None,
        }
    }

    /// What is wrong with the user's config file, when it is there and cannot
    /// be read. The rule of start-up holds for every agent a session starts
    /// later: such a file is not replaced by the built-in default.
    pub fn config_problem() -> Option<String> {
        Self::unreadable(ShaiConfig::load_for_roles().as_ref())
    }

    /// The config an agent without its own provider runs on, loaded once.
    ///
    /// The built-in default stands in only when there is no config file. A
    /// file that is there and cannot be read is an error, as at start-up:
    /// the primary model is built from this same load, so the file cannot
    /// break between a check and the read that builds the client.
    pub fn primary_config() -> Result<ShaiConfig, String> {
        Self::primary_config_from(ShaiConfig::load_for_roles())
    }

    fn primary_config_from(config: Result<ShaiConfig, RoleError>) -> Result<ShaiConfig, String> {
        match config {
            Ok(config) => Ok(config),
            Err(RoleError::ConfigMissing) => Ok(ShaiConfig::default()),
            Err(error) => Err(Self::unreadable(Err(&error)).unwrap_or_else(|| error.to_string())),
        }
    }

    /// The handoff target the session is asked for, as things are now.
    fn resolve_prewalk(
        prewalk_into: Option<&str>,
        config: Result<&ShaiConfig, &RoleError>,
    ) -> Result<Option<shai_core::config::roles::BoundModel>, String> {
        if let Some(problem) = Self::unreadable(config) {
            return Err(problem);
        }
        resolve_prewalk(prewalk_into, config).map_err(|e| e.to_string())
    }

    /// Like [`SessionOptions::resolve`], but a failure ends the process: the
    /// reason goes to stderr and the exit code is 2. The callers of the
    /// session entry points do not propagate errors, so this is where
    /// "refuse to start" is enforced.
    pub fn resolve_or_exit(prewalk_into: Option<&str>) -> Self {
        match Self::resolve(prewalk_into) {
            Ok(options) => options,
            Err(error) => {
                eprintln!("error: {}", error);
                std::process::exit(2);
            }
        }
    }

    /// Options with `prewalk` still to come, for tests of the front-ends.
    #[cfg(test)]
    pub(crate) fn with_prewalk(prewalk: Prewalk) -> Self {
        Self {
            prewalk: Some(prewalk),
            ..Self::default()
        }
    }

    /// True once this session has recorded where its handoff went.
    #[cfg(test)]
    pub(crate) fn on_model_switched_is_recorded(&self) -> bool {
        self.handed_off.is_some()
    }

    /// True while a handoff is still to come.
    #[cfg(test)]
    pub(crate) fn prewalk_pending(&self) -> bool {
        self.prewalk
            .as_ref()
            .is_some_and(|prewalk| !prewalk.is_spent())
    }

    /// Arm `agent` with what this session was asked for.
    ///
    /// Returns a line for the user when there is something to say: when a
    /// handoff is armed for the first time or for a new target, and whenever
    /// one that is still to come cannot be armed on this agent.
    pub fn arm(&mut self, agent: &mut AgentCore) -> Option<String> {
        // A handoff that happened without this session being told is
        // accounted for here at the latest.
        self.note_switch();
        let Some(prewalk) = self.prewalk.clone() else {
            // An agent started after the handoff runs on its own model. The
            // last thing said was that the conversation had moved, so say it.
            return self.handed_off.as_deref().map(prewalk_over_notice);
        };
        match agent.arm_prewalk(prewalk.clone()) {
            Ok(()) => {
                let label = prewalk.target().label();
                if self.announced.as_deref() == Some(label.as_str()) {
                    return None;
                }
                let notice = prewalk_armed_notice(&label);
                self.announced = Some(label);
                Some(notice)
            }
            Err(error) => {
                self.announced = None;
                if error == PrewalkArmError::Spent {
                    // Taken by an earlier agent whose brain then failed to
                    // switch, as that agent reported. It is not tried again.
                    self.prewalk = None;
                }
                Some(prewalk_not_armed_notice(&error.to_string()))
            }
        }
    }

    /// Account for a handoff that has happened: the session keeps where it
    /// went and holds nothing to arm any more. Returns the handoff if this
    /// is the first the session hears of it.
    fn note_switch(&mut self) -> Option<Prewalk> {
        if !self.prewalk.as_ref()?.has_switched() {
            return None;
        }
        let prewalk = self.prewalk.take()?;
        self.handed_off = Some(prewalk.target().label());
        Some(prewalk)
    }

    /// An agent has announced that it moved to `provider`/`model`. Returns
    /// false if this session had already accounted for the handoff, in which
    /// case the announcement is a repeat.
    pub fn on_model_switched(&mut self, provider: &str, model: &str) -> bool {
        if self.prewalk.take().is_none() {
            return false;
        }
        self.handed_off = Some(format!("{provider}/{model}"));
        true
    }

    /// The announcement of a handoff that has happened without this session
    /// being told: the event was lost, or the agent was replaced before its
    /// events were read. The handoff is accounted for from then on.
    pub fn take_unseen_switch(&mut self) -> Option<AgentEvent> {
        let prewalk = self.note_switch()?;
        Some(AgentEvent::ModelSwitched {
            provider: prewalk.target().provider().to_string(),
            model: prewalk.target().model().to_string(),
        })
    }

    /// Resolve the options again, after the config may have changed while
    /// the session was running.
    ///
    /// A handoff that is still to come follows the config: it is aimed at
    /// what its target resolves to now, and given up if that no longer
    /// resolves. It is never left bound to credentials or an endpoint the
    /// config no longer holds. Returns lines to show the user.
    pub fn refresh(&mut self) -> Vec<String> {
        let config = ShaiConfig::load_for_roles();
        self.refresh_from(config.as_ref())
    }

    fn refresh_from(&mut self, config: Result<&ShaiConfig, &RoleError>) -> Vec<String> {
        let Some(current) = self.prewalk.clone().filter(|prewalk| !prewalk.is_spent()) else {
            return Vec::new();
        };
        let reason = match Self::resolve_prewalk(self.prewalk_into.as_deref(), config) {
            Ok(Some(target)) => {
                // `arm` announces it again if it is not the same model.
                self.prewalk = Some(current.retarget(target));
                return Vec::new();
            }
            Ok(None) => "the config no longer names a target".to_string(),
            Err(error) => error,
        };
        self.prewalk = None;
        self.announced = None;
        vec![prewalk_disarmed_notice(&reason)]
    }
}

/// Handoffs carried out by a real agent, for tests of the front-ends.
#[cfg(test)]
pub(crate) mod test_support {
    use async_trait::async_trait;
    use openai_dive::v1::resources::chat::{ChatMessage, Function, ToolCall};
    use serde_json::json;
    use shai_core::agent::{
        Agent, AgentBuilder, AgentError, Brain, Prewalk, ThinkerContext, ThinkerDecision,
        ToolBudgetRef,
    };
    use shai_core::tools::FsOperationLog;
    use shai_llm::LlmClient;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::time::Duration;

    /// A brain that replays tool calls and then stops. It talks to no model.
    /// It says it can continue on another one; `switches` is whether it then
    /// does.
    struct ReplayBrain {
        steps: VecDeque<(&'static str, serde_json::Value)>,
        switches: bool,
    }

    #[async_trait]
    impl Brain for ReplayBrain {
        async fn next_step(
            &mut self,
            _: ThinkerContext,
            _: ToolBudgetRef,
        ) -> Result<ThinkerDecision, AgentError> {
            let tool_calls = self.steps.pop_front().map(|(name, arguments)| {
                vec![ToolCall {
                    id: format!("call_{name}"),
                    r#type: "function".to_string(),
                    function: Function {
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    },
                }]
            });
            Ok(ThinkerDecision::agent_pause(ChatMessage::Assistant {
                content: None,
                reasoning: None,
                reasoning_content: None,
                tool_calls,
                name: None,
                audio: None,
                refusal: None,
            }))
        }

        fn switch_model(&mut self, _: Arc<LlmClient>, _: String) -> bool {
            self.switches
        }

        fn can_switch_model(&self) -> bool {
            true
        }
    }

    /// Let a real agent, armed with `prewalk`, plan and write a file, so
    /// that the handoff comes due. With `switches` the agent then moves to
    /// the target; without, its brain fails to. Nobody reads its events.
    pub(crate) async fn hand_off(prewalk: &Prewalk, switches: bool) {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("out.txt");
        let (tools, todos) = AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new()));
        let brain = ReplayBrain {
            steps: [
                (
                    "todo_write",
                    json!({"todos": [{"content": "write the file", "status": "in_progress"}]}),
                ),
                (
                    "write",
                    json!({"files": [{"path": file.to_string_lossy(), "content": "x"}]}),
                ),
            ]
            .into(),
            switches,
        };
        let mut agent = AgentBuilder::with_brain(Box::new(brain))
            .tools(tools)
            .set_todo_storage(todos)
            .goal("create the file")
            .sudo()
            .build();
        agent.arm_prewalk(prewalk.clone()).unwrap();
        tokio::time::timeout(Duration::from_secs(60), agent.run())
            .await
            .expect("the agent run should finish")
            .expect("the agent run should succeed");
        assert!(file.exists());
        assert!(prewalk.is_spent());
        assert_eq!(prewalk.has_switched(), switches);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shai_core::agent::AgentBuilder;
    use shai_core::runners::coder::coder::CoderBrain;
    use shai_core::tools::FsOperationLog;
    use shai_llm::LlmClient;
    use std::sync::Arc;

    /// A config with a keyed `openai` entry and, optionally, an `ollama`
    /// entry at `ollama_url` that the role `cheap` points at.
    fn config(ollama_url: Option<&str>, tail: &str) -> ShaiConfig {
        let ollama = ollama_url
            .map(|url| {
                format!(
                    r#", {{"provider": "ollama", "env_vars": {{"OLLAMA_BASE_URL": "{url}"}}, "model": "llama3", "tool_method": "FunctionCall"}}"#
                )
            })
            .unwrap_or_default();
        serde_json::from_str(&format!(
            r#"{{
                "providers": [
                    {{"provider": "openai", "env_vars": {{"OPENAI_API_KEY": "k"}}, "model": "gpt-4o", "tool_method": "FunctionCall"}}
                    {ollama}
                ],
                "selected_provider": 0,
                "roles": {{"cheap": "ollama/qwen3:8b"}}
                {tail}
            }}"#
        ))
        .expect("config should load")
    }

    const URL_A: &str = "http://127.0.0.1:9/v1";

    /// An agent on the coder brain that is never run: no socket is opened.
    fn agent(without_tools: &[&str]) -> AgentCore {
        let (tools, _) = AgentBuilder::create_default_tools(Arc::new(FsOperationLog::new()));
        let tools = tools
            .into_iter()
            .filter(|tool| !without_tools.contains(&tool.name().as_str()))
            .collect();
        let llm = Arc::new(LlmClient::ollama(URL_A.to_string()));
        AgentBuilder::with_brain(Box::new(CoderBrain::new(llm, "primary".to_string())))
            .tools(tools)
            .build()
    }

    fn invalid() -> RoleError {
        RoleError::ConfigInvalid {
            path: "/home/me/.config/shai/auth.config.json".to_string(),
            reason: "JSON syntax error at line 3 column 7".to_string(),
        }
    }

    #[test]
    fn nothing_asked_for_means_nothing_to_arm() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(None, Ok(&cfg)).unwrap();
        assert!(!options.prewalk_pending());
        let mut agent = agent(&[]);
        assert_eq!(options.arm(&mut agent), None);
        assert!(!agent.is_prewalk_armed());
        assert!(options.refresh_from(Ok(&cfg)).is_empty());
        assert!(options.take_unseen_switch().is_none());

        // The same when there is no config file at all.
        let options = SessionOptions::resolve_from(None, Err(&RoleError::ConfigMissing)).unwrap();
        assert!(!options.prewalk_pending());
    }

    #[test]
    fn a_config_file_that_cannot_be_read_stops_the_session() {
        // With or without the flag: the file may ask for a handoff.
        for flag in [None, Some("cheap")] {
            let error = SessionOptions::resolve_from(flag, Err(&invalid()))
                .err()
                .expect("the session must not start");
            assert!(error.contains("auth.config.json"), "{error}");
            assert!(error.contains("line 3 column 7"), "{error}");
        }
    }

    #[test]
    fn an_unresolvable_target_stops_the_session() {
        let cfg = config(Some(URL_A), "");
        assert!(SessionOptions::resolve_from(Some("planner"), Ok(&cfg)).is_err());
        let cfg = config(Some(URL_A), r#", "prewalk": {"into": "planner"}"#);
        assert!(SessionOptions::resolve_from(None, Ok(&cfg)).is_err());
        assert!(
            SessionOptions::resolve_from(Some("cheap"), Err(&RoleError::ConfigMissing)).is_err()
        );
    }

    #[test]
    fn arming_is_announced_once_per_target() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&cfg)).unwrap();
        assert!(options.prewalk_pending());

        let mut first = agent(&[]);
        let notice = options.arm(&mut first).expect("armed for the first time");
        assert!(
            notice.starts_with("prewalk armed:") && notice.contains("ollama/qwen3:8b"),
            "{notice}"
        );
        assert!(first.is_prewalk_armed());

        // An agent that replaces it is armed too, without saying it again.
        let mut second = agent(&[]);
        assert_eq!(options.arm(&mut second), None);
        assert!(second.is_prewalk_armed());
    }

    #[test]
    fn an_agent_that_can_never_hand_off_is_not_armed_and_the_user_is_told() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&cfg)).unwrap();

        let mut no_todo = agent(&["todo_write"]);
        let notice = options.arm(&mut no_todo).expect("the user must be told");
        assert!(notice.starts_with("prewalk not armed:"), "{notice}");
        assert!(notice.contains("todo_write"), "{notice}");
        assert!(!no_todo.is_prewalk_armed());

        // Said every time, and the handoff is still there for an agent that
        // can take it, which is announced as armed.
        let mut no_edit = agent(&["write", "edit"]);
        let notice = options.arm(&mut no_edit).expect("the user must be told");
        assert!(notice.contains("write or edit"), "{notice}");
        assert!(options.prewalk_pending());
        let mut usable = agent(&[]);
        let notice = options.arm(&mut usable).expect("armed");
        assert!(notice.starts_with("prewalk armed:"), "{notice}");
    }

    #[test]
    fn the_primary_model_runs_on_the_default_only_without_a_config_file() {
        // The user's file, as loaded.
        let cfg = config(Some(URL_A), "");
        let primary = SessionOptions::primary_config_from(Ok(cfg)).unwrap();
        assert_eq!(primary.providers[0].provider, "openai");

        // No file at all: the built-in default, as before.
        let primary = SessionOptions::primary_config_from(Err(RoleError::ConfigMissing)).unwrap();
        assert_eq!(
            primary.providers[0].provider,
            ShaiConfig::default().providers[0].provider
        );

        // A file that is there and does not load is never replaced by the
        // default: the agent does not start, and the user is told why.
        let error = SessionOptions::primary_config_from(Err(invalid()))
            .map(|_| ())
            .expect_err("no default for a broken file");
        assert!(error.contains("auth.config.json"), "{error}");
        assert!(error.contains("line 3 column 7"), "{error}");
    }

    #[test]
    fn a_changed_config_re_aims_or_gives_up_the_handoff() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&cfg)).unwrap();
        let mut before = agent(&[]);
        assert!(options.arm(&mut before).is_some());

        // The same config: nothing to say, still armed.
        assert!(options.refresh_from(Ok(&cfg)).is_empty());
        assert!(options.prewalk_pending());

        // The role now points at another model: armed again, and announced.
        let other_model: ShaiConfig = serde_json::from_str(
            &serde_json::to_string(&cfg)
                .unwrap()
                .replace("ollama/qwen3:8b", "ollama/phi4"),
        )
        .unwrap();
        assert!(options.refresh_from(Ok(&other_model)).is_empty());
        let notice = options.arm(&mut agent(&[])).expect("a new target");
        assert!(notice.contains("ollama/phi4"), "{notice}");

        // The provider entry is gone: the handoff is given up, visibly, and
        // the next agent is not armed with the client bound at start-up.
        let removed = config(None, "");
        let notices = options.refresh_from(Ok(&removed));
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].starts_with("prewalk disarmed") && notices[0].contains("not configured"),
            "{notices:?}"
        );
        assert!(!options.prewalk_pending());
        let mut after = agent(&[]);
        assert_eq!(options.arm(&mut after), None);
        assert!(!after.is_prewalk_armed());

        // Given up for good: a later change does not bring it back.
        assert!(options.refresh_from(Ok(&cfg)).is_empty());
        assert!(!options.prewalk_pending());
    }

    #[test]
    fn a_config_that_stops_loading_or_asking_gives_up_the_handoff() {
        let asking = config(Some(URL_A), r#", "prewalk": {"into": "cheap"}"#);

        let mut options = SessionOptions::resolve_from(None, Ok(&asking)).unwrap();
        assert!(options.prewalk_pending());
        let notices = options.refresh_from(Err(&invalid()));
        assert!(notices[0].contains("could not be loaded"), "{notices:?}");
        assert!(!options.prewalk_pending());

        let mut options = SessionOptions::resolve_from(None, Ok(&asking)).unwrap();
        let notices = options.refresh_from(Ok(&config(Some(URL_A), "")));
        assert!(
            notices[0].contains("no longer names a target"),
            "{notices:?}"
        );
        assert!(!options.prewalk_pending());

        // A target given on the command line does not depend on the section.
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&asking)).unwrap();
        assert!(options
            .refresh_from(Ok(&config(Some(URL_A), "")))
            .is_empty());
        assert!(options.prewalk_pending());
    }

    #[test]
    fn a_switch_is_accounted_for_once() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&cfg)).unwrap();
        // Nothing has happened: nothing unseen.
        assert!(options.take_unseen_switch().is_none());
        assert!(options.prewalk_pending());

        // The agent's own announcement arrives: news once, a repeat after.
        assert!(options.on_model_switched("ollama", "qwen3:8b"));
        assert!(!options.on_model_switched("ollama", "qwen3:8b"));
        assert!(!options.prewalk_pending());
        assert!(options.take_unseen_switch().is_none());

        // An agent started after that is not armed, and is not on the
        // target either: the user is told, every time.
        for _ in 0..2 {
            let mut later = agent(&[]);
            let notice = options.arm(&mut later).expect("the user must be told");
            assert!(
                notice.starts_with("prewalk is over:") && notice.contains("ollama/qwen3:8b"),
                "{notice}"
            );
            assert!(!later.is_prewalk_armed());
        }
        // A config change does not bring the handoff back.
        assert!(options.refresh_from(Ok(&cfg)).is_empty());
        assert!(options
            .arm(&mut agent(&[]))
            .is_some_and(|n| n.starts_with("prewalk is over:")));
    }

    #[tokio::test]
    async fn a_switch_nobody_announced_is_accounted_for_when_the_next_agent_is_armed() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&cfg)).unwrap();
        let handoff = options.prewalk.clone().expect("a handoff to come");
        test_support::hand_off(&handoff, true).await;

        // Headless mode and any path that skipped `take_unseen_switch`: the
        // next agent is still not armed, and the user is told why.
        let mut next = agent(&[]);
        let notice = options.arm(&mut next).expect("the user must be told");
        assert!(
            notice.starts_with("prewalk is over:") && notice.contains("ollama/qwen3:8b"),
            "{notice}"
        );
        assert!(!next.is_prewalk_armed());
        assert!(options.take_unseen_switch().is_none());
    }

    #[tokio::test]
    async fn a_handoff_that_failed_is_not_tried_again_and_the_user_is_told() {
        let cfg = config(Some(URL_A), "");
        let mut options = SessionOptions::resolve_from(Some("cheap"), Ok(&cfg)).unwrap();
        let handoff = options.prewalk.clone().expect("a handoff to come");
        // An agent takes the handoff and its brain then fails to switch.
        test_support::hand_off(&handoff, false).await;

        // That is not a switch, and is not announced as one.
        assert!(options.take_unseen_switch().is_none());
        assert!(!options.on_model_switched_is_recorded());

        // The next agent is not armed, with the reason, once.
        let mut next = agent(&[]);
        let notice = options.arm(&mut next).expect("the user must be told");
        assert!(
            notice.starts_with("prewalk not armed:") && notice.contains("already tried"),
            "{notice}"
        );
        assert!(!next.is_prewalk_armed());
        assert_eq!(options.arm(&mut agent(&[])), None);
        assert!(options.refresh_from(Ok(&cfg)).is_empty());
    }
}
