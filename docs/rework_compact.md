1. Brain & ThinkerContext: Purpose and Separation of Concerns
Current Architecture
AgentCore (orchestrator)
├── brain: Arc<RwLock<Box<dyn Brain>>>
├── trace / full_trace: Arc<RwLock<Vec<ChatMessage>>>
├── compaction_config: CompactionConfig
├── available_tools: Vec<Arc<dyn AnyTool>>
├── method, temperature, state machine...
│
├── spawn_next_step()
│   ├── Clones fields into ThinkerContext
│   └── Spawns brain.write().next_step(context)
│
└── process_next_step(result)
    └── Pushes message, spawns tools, manages state

ThinkerContext { trace, available_tools, method, max_trace_chars, temperature }

CoderBrain::next_step(context)
├── Clones trace from context.trace.read()
├── compact_trace_if_needed(&mut trace, context.max_trace_chars)  // char-based
├── Renders system prompt
├── Inserts system prompt at trace[0]
├── Calls self.llm.chat_with_tools()
└── Returns ThinkerDecision
Problems
1. max_trace_chars leaks compaction config into ThinkerContext — the Brain shouldn't know about compaction. It's an orchestration concern.
2. CompactionConfig lives on AgentCore but compaction logic runs in CoderBrain — config and logic are split across two layers, connected only by max_trace_chars.
3. ThinkerContext.trace is Arc<RwLock<...>> — but next_step() immediately clones it (context.trace.read().await.clone()). An owned Vec<ChatMessage> snapshot would be simpler.
4. ContextCompressor is dead code — fully implemented but never instantiated.
5. CoderBrain::next_step() does too much — system prompt rendering, trace compaction, tool resolution, and LLM calling are all jam into one method.
Proposed Rework
Option A — Move compaction to AgentCore (cleaner, more invasive):
AgentCore
├── compaction_manager: CompactionManager
│
└── spawn_next_step()
    ├── Compress trace (compaction_manager.compress_if_needed)
    ├── Snapshot trace → Vec<ChatMessage>
    └── Construct ThinkerContext { trace, tools, method, temperature }

ThinkerContext {
    trace: Vec<ChatMessage>,           // owned snapshot
    available_tools: AnyToolBox,
    method: ToolCallMethod,
    temperature: f32,
}

CoderBrain::next_step(context)
├── Render system prompt
├── Call LLM
└── Return decision
The Brain becomes purely "given a clean trace, decide what to do." Compaction is AgentCore's responsibility.
Problem: CompactionManager needs LlmClient for summary generation, but AgentCore doesn't currently hold one — only CoderBrain does.
Option B — Keep compaction in Brain, pass full config (less invasive):
ThinkerContext {
    trace: Vec<ChatMessage>,           // owned snapshot
    available_tools: AnyToolBox,
    method: ToolCallMethod,
    temperature: f32,
}

CoderBrain {
    llm: Arc<LlmClient>,
    model: String,
    compaction_manager: CompactionManager,  // new field
    ...
}
CompactionManager is created in CoderBrain::new() / with_custom_prompt() using CompactionConfig. ThinkerContext drops max_trace_chars entirely.
My recommendation: Option B is more practical — it avoids adding LlmClient to AgentCore and keeps the change contained to CoderBrain + ThinkerContext. Option A would be cleaner long-term but requires more plumbing.
2. Token-Based Compression Flow
Desired Flow
max_context_tokens from CompactionConfig
    │
    ├── Try loading HuggingFace tokenizer (TokenCounter::Local)
    │   └── If loaded: count tokens → compress_if_needed() via ContextCompressor
    │
    ├── Fall back to API-based counting (TokenCounter::Api)
    │   └── Uses last prompt_tokens from LLM response
    │   └── If token count available: compress_if_needed() via ContextCompressor
    │
    └── Fall back to char-based compact_trace_if_needed()
        └── Uses max_trace_chars threshold
CompactionManager (new)
pub struct CompactionManager {
    token_counter: TokenCounter,
    max_context_tokens: u32,
    compression_threshold: f32,
    keep_recent_messages: usize,
    max_trace_chars: usize,
}

impl CompactionManager {
    pub fn new(config: &CompactionConfig, model: &str) -> Self {
        let token_counter = TokenCounter::new(model, config.tokenizer_model.as_deref());
        let max_context_tokens = config.max_context_tokens.unwrap_or(128_000);
        Self { token_counter, max_context_tokens, ... }
    }

    pub async fn compress_if_needed(
        &self,
        trace: &mut Vec<ChatMessage>,
        llm: &Arc<LlmClient>,
        model: &str,
    ) -> Option<CompressionInfo> {
        // 1. Try token-based compression
        if let Some(token_count) = self.token_counter.count_messages(trace).await {
            let threshold = (self.max_context_tokens as f32 * self.compression_threshold) as u32;
            if token_count < threshold {
                return None; // No compression needed
            }
            // Use ContextCompressor for LLM-based summary compression
            return ContextCompressor::new(
                Some(self.max_context_tokens),
                self.compression_threshold,
                self.keep_recent_messages,
                model,
                None,
            ).compress_force(std::mem::take(trace), llm, model).await.map(|(new_trace, info)| {
                *trace = new_trace;
                Some(info)
            }).flatten();
        }

        // 2. Fall back to char-based compression
        if self.max_trace_chars > 0 {
            compact_trace_if_needed(trace, self.max_trace_chars);
        }
        None
    }
}
Integration into CoderBrain::next_step()
async fn next_step(&mut self, context: ThinkerContext) -> Result<ThinkerDecision, AgentError> {
    let mut trace = context.trace.read().await.clone();

    // Compress if needed (token-based with char-based fallback)
    self.compaction_manager.compress_if_needed(&mut trace, &self.llm, &self.model).await;

    // ... rest of next_step (system prompt, LLM call, etc.)
}
3. Custom Config for the Benchmark Harness
Problem
The harness (measurement_harness.rs:419-433) uses ShaiConfig::get_llm() + coder() factory, bypassing AgentConfig entirely. There's no way to customize compaction settings, system prompts, or tool selection for benchmarks.
Proposed Changes
Add AgentConfig::load_from_path() (shai-core/src/config/agent.rs):
pub fn load_from_path(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
    let content_bytes = std::fs::read(path)?;
    let content_stripped = StripComments::new(&content_bytes[..]);
    let config: AgentConfig = serde_json::from_reader(content_stripped)?;
    Ok(config)
}
Add --config <path> flag to the harness:
// In parse_args():
let mut config_path: Option<String> = None;
// ... handle --config <path> ...

// In main():
let agent = if let(path) = config_path {
    let config = AgentConfig::load_from_path(&PathBuf::from(path))
        .map_err(|e| format!("Failed to load agent config: {}", e))?;
    AgentBuilder::from_config(config).await?.build()
} else {
    coder(Arc::new(llm), model.clone())
};
Example benchmark config (benchmark-agent.config):
{
  "name": "benchmark-agent",
  "description": "Agent for benchmark testing",
  "llm_provider": {
    "provider": "ovhcloud",
    "env_vars": {
      "OVH_BASE_URL": "https://gpt-oss-120b.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1"
    },
    "model": "gpt-oss-120b",
    "tool_method": "FunctionCall"
  },
  "tools": { "builtin": ["*"] },
  "system_prompt": "{{CODER_BASE_PROMPT}}",
  "max_tokens": 4096,
  "temperature": 0.0,
  "compaction": {
    "enabled": true,
    "max_context_tokens": 8000,
    "compression_threshold": 0.80,
    "keep_recent_messages": 6,
    "max_trace_chars": 50000
  }
}
./measurement_harness --config ./benchmark-agent.config script.json
