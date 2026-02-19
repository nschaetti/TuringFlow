# CLI Usage

`turingflow` currently exposes eight commands.

## Help

```bash
cargo run --bin turingflow -- --help
```

## `image`

Generate a structured response from an image prompt.

Example:

```bash
cargo run --bin turingflow -- image \
  --image agent_tests/example.png \
  --prompt "Describe this image" \
  --config config/test.yaml \
  --format json
```

Optional output file:

```bash
--output out.json
```

## `embeddings`

Generate an embedding vector from a text file.

```bash
cargo run --bin turingflow -- embeddings \
  --text agent_tests/example.txt \
  --model nomic-ai/nomic-embed-text-v1.5
```

## `calc`

Tool-calling demo (`multiply` function).

```bash
cargo run --bin turingflow -- calc \
  --prompt "What is 12 times 9?" \
  --model accounts/fireworks/models/minimax-m2p1 \
  --temperature 0.0
```

## `chat`

Queue a user message for agents (user-plane ingress).

```bash
cargo run --bin turingflow -- chat \
  --message "Planifie ma semaine" \
  --channel cli \
  --thread-id user-main
```

## `inbox`

Read asynchronous messages posted by agents for the user.

```bash
cargo run --bin turingflow -- inbox --limit 20
```

Include already delivered records:

```bash
--include-delivered
```

## `debug-user`

Show user-plane queues directly from SQLite for end-to-end debugging.

```bash
cargo run --bin turingflow -- debug-user --limit 50 --include-acked --include-delivered
```

## `test_agent2`

Runs the multimodal agentic demo (tool loop + image inspection) equivalent to
`python/example_langchain_agent2_fireworks.py`.

```bash
FIREWORKS_API_KEY=... cargo run --bin turingflow -- test_agent2
```

Optional overrides:

```bash
--model accounts/fireworks/models/minimax-m2p1 \
--vision-model accounts/fireworks/models/kimi-k2p5 \
--images-dir agent_tests \
--report-path report.txt \
--recursion-limit 20
```

## `test_agent2_openai`

Runs the same multimodal agentic demo through an OpenAI-compatible chat endpoint.

Required environment:

```bash
export OPENAI_API_KEY=...
```

Optional endpoint override (for compatible providers):

```bash
export OPENAI_BASE_URL=https://your-provider.example/v1/chat/completions
```

Run:

```bash
cargo run --bin turingflow -- test_agent2_openai
```

## Security note

Tooling access (filesystem and user communication plane) is routed through kernel wrappers (`ToolRuntime`) and policy checks.
