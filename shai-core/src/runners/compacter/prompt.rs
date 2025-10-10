static COMPRESSION_SUMMARY_PROMPT: &str = r#"You are compressing conversation history. Your summary will replace older messages, so it must contain ALL information needed to continue the task.

**CRITICAL RULES:**
1. Extract the FIRST user message from the conversation and reproduce it EXACTLY (word for word)
2. List every file that was read, with key data extracted from each file
3. List every action taken (reads, edits, tool calls, reasoning)
4. Identify what the assistant was doing and what remains to be done
5. Be factual and complete - losing information breaks the conversation flow

**FORMAT YOUR SUMMARY LIKE THIS:**

**Original user request (verbatim):**
"[exact first user message here]"

**Actions completed:**
- Read file X: [key content/data from file]
- Read file Y: [key content/data from file]
- [any other actions taken]

**Key information extracted:**
- [important data point 1]
- [important data point 2]
- [etc.]

**Current state:**
[What was being done at the end of this conversation segment]

**Next steps:**
[What remains to be done to complete the user's request]

**EXAMPLE:**
If the user said "read file.txt and summarize it", and the assistant read the file containing "Hello World", your summary should be:

**Original user request (verbatim):**
"read file.txt and summarize it"

**Actions completed:**
- Read file.txt: Contains "Hello World"

**Key information extracted:**
- file.txt content: "Hello World"

**Current state:**
File has been read, summary needs to be provided to user

**Next steps:**
Provide summary of file.txt to the user

---
"#;

pub fn get_compression_summary_prompt() -> String {
    COMPRESSION_SUMMARY_PROMPT.to_string()
}