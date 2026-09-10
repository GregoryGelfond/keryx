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
violates("keryx.p2.Order", P) :- emit_order(P), id(P, _), not order(P).
%! occupancy of emit_order : order  (root)
violates("keryx.p2.Order", P) :- emit_order(P), quantity(P, _), not order(P).
%! occupancy of emit_order : order  (root)
violates("keryx.p2.Order", P) :- emit_order(P), tags(P, _, _), not order(P).
%! functionality of id : order -> string  (required)
violates("keryx.p2.Order.id", P) :- id(P, V1), id(P, V2), order(P), reach(P), V1 != V2.
%! totality of id : order -> string  (required)
violates("keryx.p2.Order.id", P) :- order(P), reach(P), not has_id(P).
%! functionality of quantity : order -> int32  (partial)
violates("keryx.p2.Order.quantity", P) :- order(P), quantity(P, V1), quantity(P, V2), reach(P), V1 != V2.
%! contiguity of tags : order × index -> string  (sequence)
violates("keryx.p2.Order.tags", P) :- has_tags(P, I), order(P), reach(P), I > 0, not has_tags(P, (I - 1)).
%! contiguity of tags : order × index -> string  (sequence)
violates("keryx.p2.Order.tags", P) :- order(P), reach(P), tags(P, I, _), I < 0.
%! emit_order : order  (root)
#defined emit_order/1.
