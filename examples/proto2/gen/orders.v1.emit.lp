#include "orders.v1.core.lp".
%! contiguity of orders : ledger × index -> order  (sequence)
has_orders(P, I) :- order(orders(P, I)).
%! contiguity of tags : order × index -> string  (sequence)
has_tags(P, I) :- tags(P, I, _).
%! membership of enum grade/1  (closed)
ok_grade(a).
%! membership of enum grade/1  (closed)
ok_grade(b).
%! membership of enum grade/1  (closed)
ok_grade(unspecified).
%! orders : ledger × index -> order  (sequence)
reach(A) :- ledger(X), order(A), reach(X), A = orders(X, I).
%! emit_ledger : ledger  (root)
reach(X) :- emit_ledger(X).
%! emit_order : order  (root)
reach(X) :- emit_order(X).
%! occupancy of emit_ledger : ledger  (root)
:- emit_ledger(P), order(orders(P, _)), not ledger(P).
%! occupancy of emit_order : order  (root)
:- emit_order(P), grade(P, _), not order(P).
%! occupancy of emit_order : order  (root)
:- emit_order(P), id(P, _), not order(P).
%! occupancy of emit_order : order  (root)
:- emit_order(P), quantity(P, _), not order(P).
%! occupancy of emit_order : order  (root)
:- emit_order(P), tags(P, _, _), not order(P).
%! membership of grade : order -> grade  (partial)
:- grade(P, V), order(P), reach(P), not ok_grade(V).
%! functionality of grade : order -> grade  (partial)
:- grade(P, V1), grade(P, V2), order(P), reach(P), V1 != V2.
%! occupancy of orders : ledger × index -> order  (sequence)
:- grade(orders(X, I), _), ledger(X), reach(X), not order(orders(X, I)).
%! contiguity of orders : ledger × index -> order  (sequence)
:- has_orders(P, I), ledger(P), reach(P), I > 0, not has_orders(P, (I - 1)).
%! contiguity of tags : order × index -> string  (sequence)
:- has_tags(P, I), order(P), reach(P), I > 0, not has_tags(P, (I - 1)).
%! functionality of id : order -> string  (partial)
:- id(P, V1), id(P, V2), order(P), reach(P), V1 != V2.
%! occupancy of orders : ledger × index -> order  (sequence)
:- id(orders(X, I), _), ledger(X), reach(X), not order(orders(X, I)).
%! contiguity of orders : ledger × index -> order  (sequence)
:- ledger(P), order(orders(P, I)), reach(P), I < 0.
%! occupancy of orders : ledger × index -> order  (sequence)
:- ledger(X), quantity(orders(X, I), _), reach(X), not order(orders(X, I)).
%! occupancy of orders : ledger × index -> order  (sequence)
:- ledger(X), reach(X), tags(orders(X, I), _, _), not order(orders(X, I)).
%! functionality of quantity : order -> int32  (partial)
:- order(P), quantity(P, V1), quantity(P, V2), reach(P), V1 != V2.
%! contiguity of tags : order × index -> string  (sequence)
:- order(P), reach(P), tags(P, I, _), I < 0.
%! emit_ledger : ledger  (root)
#defined emit_ledger/1.
%! emit_order : order  (root)
#defined emit_order/1.
