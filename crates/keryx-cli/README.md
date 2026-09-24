# keryx-cli

The `keryx` command — a thin frontend over [`keryx-core`](../keryx-core/). It compiles schemas (`gen`, `explain`), shreds messages to facts (`facts`), reassembles answer sets to messages (`emit`), and diffs schema versions (`diff`). It invokes no solver.

```sh
cargo run -p keryx-cli -- --help
```

See **the keryx Book** — [Using the keryx command](../../docs/book/reference/commands.md) — for every command and its flags.
