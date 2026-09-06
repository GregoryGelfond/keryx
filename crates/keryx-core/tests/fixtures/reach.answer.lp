% An answer set over the vocabulary of `reach.proto` that the strict theory admits (spec
% §12.2), solved by the grounding gate (`scripts/ground.sh`): one exported plan holding a step
% in every message-typed form — a singular field, a sequence, a map, and a oneof arm — each
% step carrying the `note` it owes, one nested through the child's own slot; and an unexported
% plan whose step lacks its note, which the theory must leave alone.
emit_plan(p0).
plan(p0).
step(first(p0)).            note(first(p0), "first").
step(steps(p0, 0)).         note(steps(p0, 0), "zero").
step(steps(p0, 1)).         note(steps(p0, 1), "one").
step(by_name(p0, "z")).     note(by_name(p0, "z"), "zed").
step(pick(p0)).             note(pick(p0), "picked").
step(next(first(p0))).      note(next(first(p0)), "after first").
plan(p9).  step(first(p9)).
