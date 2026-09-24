# About this book

keryx compiles a Protocol Buffers schema into an Answer Set Programming vocabulary and translates messages into ground facts — and answer sets back into messages. The message side never learns ASP; the model side never learns the wire. keryx does not solve: a consuming tool brings its own solver (clingo today) and composes keryx's translation on both sides.

This book covers using the `keryx` command, the translation model it implements, and embedding keryx as a Rust library.

## Start here

| I want to… | Read |
|---|---|
| See the whole idea in one worked example | [A guided tour](guide/tour.md) |
| Understand how a schema becomes a vocabulary | [The translation model](guide/translation-model.md) |
| Turn a message into facts | [Payloads to facts](guide/payload-formats.md) |
| Turn an answer set back into a message | [Answer sets to messages](guide/outbound.md) |
| Give a field or enum a precise ASP treatment | [Annotating the mapping](guide/annotations.md) |
| See what a schema change does to a model | [Schema evolution](guide/evolution.md) |
| Call keryx from Rust | [Getting started](library/getting-started.md) |
| Look up a command | [Using the keryx command](reference/commands.md) |

## The two parts

**Part I — The bridge** is for the ASP author and the schema owner: what keryx generates, how a message maps to facts, how an answer set maps back, and how annotations and schema evolution behave. It draws on the worked examples in [`examples/`](https://github.com/GregoryGelfond/keryx/tree/main/examples).

**Part II — The Rust library** is for the tool builder embedding keryx: the `Mapping` a schema compiles to, the `Codec` that shreds a message to facts and reassembles it, and the diagnostics and faults keryx returns.

## Results and limits

keryx is a translation library, and only that:

- **It never solves.** keryx emits an ASP vocabulary and reads answer sets; running the solver is the consuming tool's job. There is no solver inside keryx.
- **It parses no Protocol Buffers or ASP itself** beyond two bounded, non-semantic nesting guards — a `FileDescriptorSet` is the schema interface, and answer-set text is read through the ASP layer's parser.
- **Editions (2023, 2024) are refused** with a specific diagnostic until the descriptor engine supports them; keryx branches on resolved features, not syntax era, so editions become a drop-in when the engine does.

Every claim the book makes about generated output is shown from a committed, regression-tested example, so the manual cannot drift from the tool.
