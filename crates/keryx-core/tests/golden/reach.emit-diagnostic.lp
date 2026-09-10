#include "keryx.reach.core.lp".
%! totality of note : step -> string  (total)
has_note(P) :- note(P, _).
%! contiguity of steps : plan × index -> step  (sequence)
has_steps(P, I) :- step(steps(P, I)).
%! by_name : plan × string -> step  (map)
reach(A) :- plan(X), reach(X), step(A), A = by_name(X, K).
%! first : plan -> step  (partial)
reach(A) :- plan(X), reach(X), step(A), A = first(X).
%! pick : plan -> step  (oneof choice, partial)
reach(A) :- plan(X), reach(X), step(A), A = pick(X).
%! steps : plan × index -> step  (sequence)
reach(A) :- plan(X), reach(X), step(A), A = steps(X, I).
%! next : step -> step  (partial)
reach(A) :- reach(X), step(A), step(X), A = next(X).
%! emit_plan : plan  (root)
reach(X) :- emit_plan(X).
%! emit_step : step  (root)
reach(X) :- emit_step(X).
%! occupancy of emit_plan : plan  (root)
violates("keryx.reach.Plan", P) :- emit_plan(P), label(P, _), not plan(P).
%! occupancy of emit_plan : plan  (root)
violates("keryx.reach.Plan", P) :- emit_plan(P), step(by_name(P, _)), not plan(P).
%! occupancy of emit_plan : plan  (root)
violates("keryx.reach.Plan", P) :- emit_plan(P), step(first(P)), not plan(P).
%! occupancy of emit_plan : plan  (root)
violates("keryx.reach.Plan", P) :- emit_plan(P), step(pick(P)), not plan(P).
%! occupancy of emit_plan : plan  (root)
violates("keryx.reach.Plan", P) :- emit_plan(P), step(steps(P, _)), not plan(P).
%! occupancy of by_name : plan × string -> step  (map)
violates("keryx.reach.Plan.by_name", by_name(X, K)) :- note(by_name(X, K), _), plan(X), reach(X), not step(by_name(X, K)).
%! occupancy of by_name : plan × string -> step  (map)
violates("keryx.reach.Plan.by_name", by_name(X, K)) :- plan(X), reach(X), step(next(by_name(X, K))), not step(by_name(X, K)).
%! exclusivity of pick | label : plan  (oneof choice)
violates("keryx.reach.Plan.choice", P) :- label(P, _), plan(P), reach(P), step(pick(P)).
%! occupancy of first : plan -> step  (partial)
violates("keryx.reach.Plan.first", first(X)) :- note(first(X), _), plan(X), reach(X), not step(first(X)).
%! occupancy of first : plan -> step  (partial)
violates("keryx.reach.Plan.first", first(X)) :- plan(X), reach(X), step(next(first(X))), not step(first(X)).
%! functionality of label : plan -> string  (oneof choice, partial)
violates("keryx.reach.Plan.label", P) :- label(P, V1), label(P, V2), plan(P), reach(P), V1 != V2.
%! occupancy of pick : plan -> step  (oneof choice, partial)
violates("keryx.reach.Plan.pick", pick(X)) :- note(pick(X), _), plan(X), reach(X), not step(pick(X)).
%! occupancy of pick : plan -> step  (oneof choice, partial)
violates("keryx.reach.Plan.pick", pick(X)) :- plan(X), reach(X), step(next(pick(X))), not step(pick(X)).
%! contiguity of steps : plan × index -> step  (sequence)
violates("keryx.reach.Plan.steps", P) :- has_steps(P, I), plan(P), reach(P), I > 0, not has_steps(P, (I - 1)).
%! contiguity of steps : plan × index -> step  (sequence)
violates("keryx.reach.Plan.steps", P) :- plan(P), reach(P), step(steps(P, I)), I < 0.
%! occupancy of steps : plan × index -> step  (sequence)
violates("keryx.reach.Plan.steps", steps(X, I)) :- note(steps(X, I), _), plan(X), reach(X), not step(steps(X, I)).
%! occupancy of steps : plan × index -> step  (sequence)
violates("keryx.reach.Plan.steps", steps(X, I)) :- plan(X), reach(X), step(next(steps(X, I))), not step(steps(X, I)).
%! occupancy of emit_step : step  (root)
violates("keryx.reach.Step", P) :- emit_step(P), note(P, _), not step(P).
%! occupancy of emit_step : step  (root)
violates("keryx.reach.Step", P) :- emit_step(P), step(next(P)), not step(P).
%! occupancy of next : step -> step  (partial)
violates("keryx.reach.Step.next", next(X)) :- note(next(X), _), reach(X), step(X), not step(next(X)).
%! occupancy of next : step -> step  (partial)
violates("keryx.reach.Step.next", next(X)) :- reach(X), step(X), step(next(next(X))), not step(next(X)).
%! functionality of note : step -> string  (total)
violates("keryx.reach.Step.note", P) :- note(P, V1), note(P, V2), reach(P), step(P), V1 != V2.
%! totality of note : step -> string  (total)
violates("keryx.reach.Step.note", P) :- reach(P), step(P), not has_note(P).
%! emit_plan : plan  (root)
#defined emit_plan/1.
%! emit_step : step  (root)
#defined emit_step/1.
