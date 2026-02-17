# CLI Usage

`turingflow` currently exposes six commands.

## Help

```bash
cargo run --bin turingflow -- --help
```

## `image`

Generate a structured response from an image prompt.

Example:

```bash
cargo run --bin turingflow -- image \
  --image examples/example.png \
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
  --text examples/example.txt \
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

## Security note

Tooling access (filesystem and user communication plane) is routed through kernel wrappers (`ToolRuntime`) and policy checks.
