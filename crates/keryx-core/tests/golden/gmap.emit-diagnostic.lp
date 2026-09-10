#include "keryx.gmap.core.lp".
%! totality of n : inner -> uint32  (total)
has_n(P) :- n(P, _).
%! by_id : holder × uint32 -> inner  (map)
reach(A) :- holder(X), inner(A), reach(X), A = by_id(X, K).
%! thing : holder -> thing  (partial)
reach(A) :- holder(X), reach(X), thing(A), A = thing(X).
%! emit_holder : holder  (root)
reach(X) :- emit_holder(X).
%! emit_inner : inner  (root)
reach(X) :- emit_inner(X).
%! occupancy of emit_holder : holder  (root)
violates("keryx.gmap.Holder", P) :- emit_holder(P), inner(by_id(P, _)), not holder(P).
%! occupancy of emit_holder : holder  (root)
violates("keryx.gmap.Holder", P) :- emit_holder(P), thing(thing(P)), not holder(P).
%! key range of by_id : holder × uint32 -> inner  (map)
violates("keryx.gmap.Holder.by_id", P) :- holder(P), inner(by_id(P, K)), reach(P), K < 0.
%! occupancy of by_id : holder × uint32 -> inner  (map)
violates("keryx.gmap.Holder.by_id", by_id(X, K)) :- holder(X), n(by_id(X, K), _), reach(X), not inner(by_id(X, K)).
%! occupancy of emit_inner : inner  (root)
violates("keryx.gmap.Inner", P) :- emit_inner(P), n(P, _), not inner(P).
%! range of n : inner -> uint32  (total)
violates("keryx.gmap.Inner.n", P) :- inner(P), n(P, V), reach(P), V < 0.
%! functionality of n : inner -> uint32  (total)
violates("keryx.gmap.Inner.n", P) :- inner(P), n(P, V1), n(P, V2), reach(P), V1 != V2.
%! totality of n : inner -> uint32  (total)
violates("keryx.gmap.Inner.n", P) :- inner(P), reach(P), not has_n(P).
%! emit_holder : holder  (root)
#defined emit_holder/1.
%! emit_inner : inner  (root)
#defined emit_inner/1.
