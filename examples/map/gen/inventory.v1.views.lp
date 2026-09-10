#include "inventory.v1.core.lp".
%! bins : warehouse × string -> bin  (map)
bins(P, K, E) :- bin(E), E = bins(P, K).
%! featured : warehouse -> sku  (partial)
featured(P, A) :- sku(A), A = featured(P).
