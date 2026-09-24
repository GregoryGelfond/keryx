# The config example — solve a problem, get protobuf back

The other examples show the proto→asp direction. This one closes the loop: a Protocol Buffers
message in, a **solve**, and a *different* Protocol Buffers message out — the whole motion a
one-way shim cannot do.

**keryx never runs the solver.** It wires the two ends — `keryx facts` (proto→asp) and
`keryx emit` (asp→proto). The solve in between is *your* clingo, over the vocabulary keryx
generated. Here that solve is a small configuration validator.

## The two messages

[`deployment.proto`](deployment.proto) declares the message that goes **in** and the one that
comes **out**:

```proto
syntax = "proto3";
package deploy.v1;

message Deployment { repeated Service services = 1; }   // in
message Service { string name = 1; int32 replicas = 2; int32 max_per_node = 3; }

message Report  { map<string, Finding> findings = 1; }  // out, keyed by service name
message Finding { string reason = 1; int32 code = 2; }
```

## The pipeline

```
Deployment ──keryx facts──▶ facts.lp ──your clingo (model.lp)──▶ answer set ──keryx emit──▶ Report
 (protobuf in)              (proto→asp)   (keryx runs no solver)                (asp→proto)  (protobuf out)
```

1. **`keryx facts`** shreds (breaks down into flat ground facts) the `Deployment`.
2. **your clingo** runs [`model.lp`](model.lp) — the *consuming tool's* invariants — over those
   facts, deriving a `Report`.
3. **`keryx emit`** reassembles that `Report` into protobuf.

## The model (yours, not keryx's)

[`model.lp`](model.lp) is written against the vocabulary `keryx gen` produced. It has no choice
rules, so it is deterministic — one `Deployment` in, one `Report` out:

```prolog
% Invariants over the shredded Deployment facts:
bad_replicas(S) :- replicas(S, R), R < 1.
bad_capacity(S) :- replicas(S, R), max_per_node(S, M), R > M.

finding_for(S, "replicas must be at least 1", 1) :- bad_replicas(S).
finding_for(S, "replicas exceeds max_per_node", 2) :- bad_capacity(S), not bad_replicas(S).

% Build the Report keryx will emit, keyed by service name:
report(v0).
finding(findings(v0, N)) :- finding_for(S, _, _), name(S, N).
reason(findings(v0, N), R) :- finding_for(S, R, _), name(S, N).
code(findings(v0, N), C) :- finding_for(S, _, C), name(S, N).
emit_report(v0).
```

## Run it

```sh
keryx gen deployment.proto -I . -o gen/
keryx facts --root Deployment=config.bad.txtpb deployment.proto -I . > facts.bad.lp
clingo gen/deploy.v1.core.lp facts.bad.lp model.lp        # your solver, not keryx
keryx emit --root Report answer.bad.lp deployment.proto -I . > report.bad.binpb
```

[`config.bad.txtpb`](config.bad.txtpb) has three services — `web` (0 replicas), `cache` (5
replicas, max 2), and a valid `api`. clingo derives a `Report`;
[`answer.bad.lp`](answer.bad.lp) is that answer set, projected to the atoms `emit` reads.
Reassembled, [`report.bad.binpb`](report.bad.binpb) is — the literal `--out txtpb` and
`--out json` forms:

```textproto
findings:[{key:"cache",value{reason:"replicas exceeds max_per_node",code:2}},{key:"web",value{reason:"replicas must be at least 1",code:1}}]
```
```json
{"findings":{"cache":{"code":2,"reason":"replicas exceeds max_per_node"},"web":{"code":1,"reason":"replicas must be at least 1"}}}
```

A `Deployment` went in; a `Report` came out — a different message, computed by your model and
serialized by keryx. The map entries are ordered by key, so the bytes are deterministic and
golden-comparable. A valid config ([`config.ok.txtpb`](config.ok.txtpb)) derives an empty
`Report` — no findings.

## Two layers, kept apart

keryx has its own `violates(path, occupant)` atom — but that is about **serializability**
("is this answer set even a valid message?", §13.3), not your domain. Your validator's findings
are its own vocabulary (`Finding`, `reason`, `code`), distinct from keryx's. keryx guarantees the
`Report` is a well-formed message; *what* the report says is your model's.

## Scope at this stage

This example demonstrates the full round trip. The asp→proto (outbound) direction is complete —
the generated `emit.lp` grounds clean under clingo, and the repository's grounding gate runs this
very solve as test infrastructure, driving the `clingo` on `PATH` (keryx spawns no solver); the
round trip closes, a `Deployment` in and the computed `Report` out.
