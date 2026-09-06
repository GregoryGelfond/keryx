#include "keryx.gmap.core.lp".
%!totality of n : inner -> uint32  (total)
has_n(P) :- n(P, _).
%!by_id : holder × uint32 -> inner  (map)
reach(A) :- holder(X), inner(A), reach(X), A = by_id(X, K).
%!thing : holder -> thing  (partial)
reach(A) :- holder(X), reach(X), thing(A), A = thing(X).
%!emit_holder : holder  (root)
reach(X) :- emit_holder(X).
%!emit_inner : inner  (root)
reach(X) :- emit_inner(X).
%!occupancy of emit_holder : holder  (root)
:- emit_holder(P), inner(by_id(P, _)), not holder(P).
%!occupancy of emit_holder : holder  (root)
:- emit_holder(P), thing(thing(P)), not holder(P).
%!occupancy of emit_inner : inner  (root)
:- emit_inner(P), n(P, _), not inner(P).
%!key range of by_id : holder × uint32 -> inner  (map)
:- holder(P), inner(by_id(P, K)), reach(P), K < 0.
%!occupancy of by_id : holder × uint32 -> inner  (map)
:- holder(X), n(by_id(X, K), _), reach(X), not inner(by_id(X, K)).
%!range of n : inner -> uint32  (total)
:- inner(P), n(P, V), reach(P), V < 0.
%!functionality of n : inner -> uint32  (total)
:- inner(P), n(P, V1), n(P, V2), reach(P), V1 != V2.
%!totality of n : inner -> uint32  (total)
:- inner(P), reach(P), not has_n(P).
%!emit_holder : holder  (root)
#defined emit_holder/1.
%!emit_inner : inner  (root)
#defined emit_inner/1.
