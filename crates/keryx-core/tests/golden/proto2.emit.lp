#include "keryx.p2.core.lp".
%! totality of id : order -> string  (required)
has_id(P) :- id(P, _).
%! contiguity of tags : order × index -> string  (sequence)
has_tags(P, I) :- tags(P, I, _).
%! membership of enum grade/1  (closed)
ok_grade(a).
%! membership of enum grade/1  (closed)
ok_grade(b).
%! membership of enum grade/1  (closed)
ok_grade(unspecified).
%! emit_order : order  (root)
reach(X) :- emit_order(X).
%! occupancy of emit_order : order  (root)
:- emit_order(P), id(P, _), not order(P).
%! occupancy of emit_order : order  (root)
:- emit_order(P), quantity(P, _), not order(P).
%! occupancy of emit_order : order  (root)
:- emit_order(P), tags(P, _, _), not order(P).
%! contiguity of tags : order × index -> string  (sequence)
:- has_tags(P, I), order(P), reach(P), I > 0, not has_tags(P, (I - 1)).
%! functionality of id : order -> string  (required)
:- id(P, V1), id(P, V2), order(P), reach(P), V1 != V2.
%! functionality of quantity : order -> int32  (partial)
:- order(P), quantity(P, V1), quantity(P, V2), reach(P), V1 != V2.
%! contiguity of tags : order × index -> string  (sequence)
:- order(P), reach(P), tags(P, I, _), I < 0.
%! totality of id : order -> string  (required)
:- order(P), reach(P), not has_id(P).
%! emit_order : order  (root)
#defined emit_order/1.
