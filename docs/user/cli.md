# CLI Usage

`turingflow` currently exposes three commands.

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

## Security note

Tooling file access is routed through kernel wrappers (`ToolRuntime`) and policy checks.
