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
%! info : order -> detail  (required)
reach(A) :- detail(A), order(X), reach(X), A = info(X).
%! emit_detail : detail  (root)
reach(X) :- emit_detail(X).
%! emit_order : order  (root)
reach(X) :- emit_order(X).
%! functionality of text : detail -> string  (partial)
:- detail(P), reach(P), text(P, V1), text(P, V2), V1 != V2.
%! occupancy of emit_order : order  (root)
:- detail(info(P)), emit_order(P), not order(P).
%! occupancy of emit_detail : detail  (root)
:- emit_detail(P), text(P, _), not detail(P).
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
%! totality of info : order -> detail  (required)
:- order(P), reach(P), not detail(info(P)).
%! totality of id : order -> string  (required)
:- order(P), reach(P), not has_id(P).
%! occupancy of info : order -> detail  (required)
:- order(X), reach(X), text(info(X), _), not detail(info(X)).
%! emit_detail : detail  (root)
#defined emit_detail/1.
%! emit_order : order  (root)
#defined emit_order/1.
