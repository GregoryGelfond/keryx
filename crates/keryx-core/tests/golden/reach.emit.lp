#include "keryx.reach.core.lp".
%!by_name : plan × string -> step  (map)
reach(A) :- plan(X), reach(X), step(A), A = by_name(X, K).
%!first : plan -> step  (partial)
reach(A) :- plan(X), reach(X), step(A), A = first(X).
%!pick : plan -> step  (oneof choice, partial)
reach(A) :- plan(X), reach(X), step(A), A = pick(X).
%!steps : plan × index -> step  (sequence)
reach(A) :- plan(X), reach(X), step(A), A = steps(X, I).
%!emit_plan : plan  (root)
reach(X) :- emit_plan(X).
%!emit_step : step  (root)
reach(X) :- emit_step(X).
%!emit_plan : plan  (root)
#defined emit_plan/1.
%!emit_step : step  (root)
#defined emit_step/1.
