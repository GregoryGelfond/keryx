#include "inventory.v1.core.lp".
%!totality of quantity : bin -> uint32  (total)
has_quantity(P) :- quantity(P, _).
%!bins : warehouse × string -> bin  (map)
reach(A) :- bin(A), reach(X), warehouse(X), A = bins(X, K).
%!featured : warehouse -> sku  (partial)
reach(A) :- reach(X), sku(A), warehouse(X), A = featured(X).
%!emit_bin : bin  (root)
reach(X) :- emit_bin(X).
%!emit_warehouse : warehouse  (root)
reach(X) :- emit_warehouse(X).
%!range of quantity : bin -> uint32  (total)
:- bin(P), quantity(P, V), reach(P), V < 0.
%!functionality of quantity : bin -> uint32  (total)
:- bin(P), quantity(P, V1), quantity(P, V2), reach(P), V1 != V2.
%!totality of quantity : bin -> uint32  (total)
:- bin(P), reach(P), not has_quantity(P).
%!occupancy of emit_warehouse : warehouse  (root)
:- bin(bins(P, _)), emit_warehouse(P), not warehouse(P).
%!occupancy of emit_bin : bin  (root)
:- emit_bin(P), quantity(P, _), not bin(P).
%!occupancy of emit_warehouse : warehouse  (root)
:- emit_warehouse(P), sku(featured(P)), not warehouse(P).
%!occupancy of bins : warehouse × string -> bin  (map)
:- quantity(bins(X, K), _), reach(X), warehouse(X), not bin(bins(X, K)).
%!emit_bin : bin  (root)
#defined emit_bin/1.
%!emit_warehouse : warehouse  (root)
#defined emit_warehouse/1.
