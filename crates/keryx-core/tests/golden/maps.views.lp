#include "keryx.maps.core.lp".
%!items : inventory × int64 -> item  (map)
items(P, K, E) :- item(E), E = items(P, K).
