# SHAI

shai is a coding agent, your pair programming buddy that lives in the terminal. Written in rust with love <3

![Shai CLI Screenshot](./docs/assets/shai.png)

## Features

- **Interactive coding agent** - Chat with shai in your terminal to write code, fix bugs, and get answers
- **Headless mode** - Pipe prompts directly into shai for scripting and automation
- **HTTP server** - Run shai as a service with OpenAI-compatible APIs and SSE streaming
- **Shell assistant** - Automatically suggests fixes when commands fail in your terminal
- **Project context** - Load project-specific information via `SHAI.md` files
- **MCP Support** - Configure specialized agents with MCP and OAuth support
- **Multiple LLM providers** - Works with OVHCloud, OpenAI, and other compatible endpoints

## Installation

### Latest stable release

Install the latest release with the following command:

```bash
curl -fsSL https://raw.githubusercontent.com/ovh/shai/main/install.sh | sh
```

### Nightly version

Install the last [``unstable``](https://github.com/ovh/shai/releases/tag/unstable) version with the following command:

```bash
curl -fsSL https://raw.githubusercontent.com/ovh/shai/main/install.sh | SHAI_RELEASE=unstable sh
```

The `shai` binary will be installed in `$HOME/.local/bin`

## Quick Start

By default `shai` uses OVHcloud as an anonymous user meaning you will be rate limited! If you want to sign in with your account or select another provider, run:

```bash
shai auth
```

![shai auth](./docs/assets/auth.gif)

Once you have a provider set up, you can run shai:

```bash
shai
```

![shai](./docs/assets/shai-hello-world.gif)

## Usage

### Interactive Mode

Simply run `shai` to start the interactive coding agent. You can chat with shai and it will help you write code, fix bugs, and answer questions.

### Headless Mode

Shai can also run in headless mode without user interface. In that case simply pipe a prompt into shai, it will stream event in the stderr:

```bash
echo "make me a hello world in main.py" | shai
```

![shai headless](./docs/assets/shai-headless.gif)

You can also instruct shai to return the entire conversation as a trace once it is done:

```bash
echo "make me a hello world in main.py" | shai 2>/dev/null --trace
```

This is handy because you can chain `shai` calls:

```bash
echo "make me a hello world in main.py" | shai --trace | shai "now run it!"
```

### HTTP Server Mode

You can run shai as an HTTP service with SSE streaming support. This mode provides multiple API endpoints:

```bash
shai serve --port 3000
```

![shai http](./docs/assets/shai-http.png)

Available API endpoints:

- **POST /v1/chat/completions** - OpenAI Chat Completions API (ephemeral mode)
- **POST /v1/responses** - OpenAI Responses API (stateful/stateless)
- **GET /v1/responses/{id}** - Get response by ID
- **POST /v1/responses/{id}/cancel** - Cancel a response
- **POST /v1/multimodal** - Simple multimodal API (streaming)
- **POST /v1/multimodal/{session_id}** - Simple multimodal API (with session)

Options:

- `--port <PORT>` - Port to bind to (default: 3000)
- `--ephemeral` - Use ephemeral mode (spawn new agent per request)
- `[AGENT]` - Agent name to use for persistent session

### Shell Assistant

shai can also act as a shell assistant in case a command failed and will propose you a fix. This works by injecting command hook while monitoring your terminal output. Your last terminal output along with the last command and error code will be sent for analysis to the llm provider.

To start hooking your shell with shai simply type:

```bash
shai on
```

For instance:

![Shai CLI Screenshot](./docs/assets/shai-shell.png)

To stop shai from monitoring your shell you can type:

```bash
shai off
```

## Configuration

### Project Context File

You can create a `SHAI.md` file at the root of your project containing any information you want Shai to know about the project (architecture, build steps, important directories, etc.). Shai will automatically load this file as additional context.

### Custom Agents (with MCP)

Instead of a single global configuration, you can create custom agent in a separate configuration.

[`.ovh.config`](./.ovh.config) contains an example of a custom configuration with an remote MCP server configured.

Place this file in `~/.config/shai/agents/ovh.config`, you can then list the agents available with:

```bash
curl https://raw.githubusercontent.com/ovh/shai/refs/heads/main/.ovh.config -o ~/.config/shai/agents/ovh.config
shai agent list
```

You can run shai with this specific agent with the `agent` subcommand:

```bash
shai agent ovh
```

### OVHCloud Endpoints

OVHCloud provides compatible LLM endpoints for using shai with tools. Start by creating a [_Public Cloud_ project in your OVHCloud account](https://www.ovh.com/manager/#/public-cloud), then head to _AI Endpoints_ and retrieve your API key. After setting it in shai, you can:

- choose [one of the models with function calling feature](https://endpoints.ai.cloud.ovh.net/catalog) (e.g., [gpt-oss-120b](https://endpoints.ai.cloud.ovh.net/models/gpt-oss-120b), [gpt-oss-20b](https://endpoints.ai.cloud.ovh.net/models/gpt-oss-20b), [Mistral-​Small-​3.2-​24B-​Instruct-​2506](https://endpoints.ai.cloud.ovh.net/models/mistral-small-3-2-24b-instruct-2506)) for best performance ;
- choose any other model forcing structured output (`/set so` option).

## Development

### Build The Project

Simply build the project with `cargo`

```bash
git clone git@github.com:ovh/shai.git
cd shai
cargo build --release
```
