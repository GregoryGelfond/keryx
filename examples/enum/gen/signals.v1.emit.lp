#include "signals.v1.core.lp".
%!totality of intersection : light -> string  (total)
has_intersection(P) :- intersection(P, _).
%!contiguity of lights : corridor × index -> light  (sequence)
has_lights(P, I) :- light(lights(P, I)).
%!totality of phase : light -> phase  (total)
has_phase(P) :- phase(P, _).
%!membership of enum phase/1  (open)
ok_phase(caution).
%!membership of enum phase/1  (open)
ok_phase(go).
%!membership of enum phase/1  (open)
ok_phase(stop).
%!membership of enum phase/1  (open)
ok_phase(unspecified).
%!lights : corridor × index -> light  (sequence)
reach(A) :- corridor(X), light(A), reach(X), A = lights(X, I).
%!emit_corridor : corridor  (root)
reach(X) :- emit_corridor(X).
%!emit_light : light  (root)
reach(X) :- emit_light(X).
%!contiguity of lights : corridor × index -> light  (sequence)
:- corridor(P), has_lights(P, I), reach(P), I > 0, not has_lights(P, (I - 1)).
%!contiguity of lights : corridor × index -> light  (sequence)
:- corridor(P), light(lights(P, I)), reach(P), I < 0.
%!occupancy of lights : corridor × index -> light  (sequence)
:- corridor(X), intersection(lights(X, I), _), reach(X), not light(lights(X, I)).
%!occupancy of lights : corridor × index -> light  (sequence)
:- corridor(X), phase(lights(X, I), _), reach(X), not light(lights(X, I)).
%!occupancy of emit_corridor : corridor  (root)
:- emit_corridor(P), light(lights(P, _)), not corridor(P).
%!occupancy of emit_light : light  (root)
:- emit_light(P), intersection(P, _), not light(P).
%!occupancy of emit_light : light  (root)
:- emit_light(P), phase(P, _), not light(P).
%!functionality of intersection : light -> string  (total)
:- intersection(P, V1), intersection(P, V2), light(P), reach(P), V1 != V2.
%!membership of phase : light -> phase  (total)
:- light(P), phase(P, V), reach(P), not ok_phase(V).
%!functionality of phase : light -> phase  (total)
:- light(P), phase(P, V1), phase(P, V2), reach(P), V1 != V2.
%!totality of intersection : light -> string  (total)
:- light(P), reach(P), not has_intersection(P).
%!totality of phase : light -> phase  (total)
:- light(P), reach(P), not has_phase(P).
%!emit_corridor : corridor  (root)
#defined emit_corridor/1.
%!emit_light : light  (root)
#defined emit_light/1.
