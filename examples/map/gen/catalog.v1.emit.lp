#include "catalog.v1.core.lp".
%! totality of code : sku -> uint32  (total)
has_code(P) :- code(P, _).
%! emit_sku : sku  (root)
reach(X) :- emit_sku(X).
%! range of code : sku -> uint32  (total)
:- code(P, V), reach(P), sku(P), V < 0.
%! functionality of code : sku -> uint32  (total)
:- code(P, V1), code(P, V2), reach(P), sku(P), V1 != V2.
%! occupancy of emit_sku : sku  (root)
:- code(P, _), emit_sku(P), not sku(P).
%! totality of code : sku -> uint32  (total)
:- reach(P), sku(P), not has_code(P).
%! emit_sku : sku  (root)
#defined emit_sku/1.
