# Security policy

## Supported versions

keryx is developed on `main`, and the supported version is the latest tagged release built from it. Fixes land on `main` and ship in the next release; there is no separate maintenance branch.

## Reporting a vulnerability

Please report a suspected vulnerability privately, not as a public issue. Use GitHub's private vulnerability reporting — the **Security** tab of the repository, then **Report a vulnerability** — which opens a private advisory visible only to the maintainer.

Include what a maintainer needs to reproduce it: the input (a schema, a payload, or an answer set), the command or library call, and what happened versus what you expected. Please allow a reasonable window for an acknowledgement and a fix before any public disclosure.

## Scope

keryx translates between Protocol Buffers and Answer Set Programming; it invokes no solver and opens the input surfaces (a schema, a payload, an answer set) that a translator must. Those surfaces, what is defended at each, and what is assumed trusted are stated in the threat model of record, [`docs/design/threat-model.md`](docs/design/threat-model.md) — the right starting point for a security review.

Reports of the following are especially in scope:

- a crash, hang, or unbounded resource use reachable from an admitted input (a schema, a payload, or an answer set) — keryx aims to refuse such input as a typed diagnostic, never to abort;
- a translation that misrepresents its input — facts that do not correspond to the message, or a reassembled message that is not what the answer set names;
- content from an input escaping into generated ASP or a diagnostic in a way that could mislead a downstream reader.

keryx pins its dependencies exactly (a git revision for the ASP layer; `=`-versions for the descriptor engine, the `.proto` compiler, and the JSON deserializer) and re-owns its hardening against each on any deliberate bump, so a report tied to a specific dependency version is welcome.
