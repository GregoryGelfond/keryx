# Changelog

All notable changes to keryx are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/), and keryx follows [Semantic Versioning](https://semver.org/).

## [1.0.0] — 2026-09-23

The first public release. keryx compiles a Protocol Buffers schema into an Answer Set Programming vocabulary, translates messages into ground facts, and reassembles answer sets back into messages — a complete, bidirectional bridge. It invokes no solver; a consuming tool brings its own and composes keryx's translation on both sides.

### Added

- **`keryx gen` / `keryx explain`** — compile a schema into the ASP vocabulary (`core.lp`, `views.lp`, `emit.lp`, and a manifest) and inspect the mapping.
- **`keryx facts`** — shred a message into ground facts, from the binary wire form, the protobuf text format, or the canonical JSON mapping.
- **`keryx emit`** — reassemble an answer set into a message, in any of the three wire forms; a byte-for-byte round trip of what `facts` produced.
- **`keryx diff`** — compare two schema versions at the level of the ASP vocabulary: a migration report, a `--json` changeset, and inbound bridge views for clean renames.
- **The annotation vocabulary** — `(keryx.set)`, `(keryx.numeric)`, `(keryx.scale)`, `(keryx.opaque)`, and `(keryx.unknown)`, each validated at the policy door.
- **Full proto2 and proto3 support** on both ends, including a proto2 `required` field's outbound totality obligation.
- **The `keryx-core` library** — `Mapping`, `Codec`, and the value plane (`Symbol`, `Name`, `Sign`) — for a tool that embeds the bridge directly.
- **The mdBook manual**, the worked examples, the threat model, and the contributor guides.

[1.0.0]: https://github.com/GregoryGelfond/keryx/releases/tag/v1.0.0
