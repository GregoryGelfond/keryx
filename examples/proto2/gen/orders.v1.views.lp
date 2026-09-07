#include "orders.v1.core.lp".
%!orders : ledger × index -> order  (sequence)
orders(P, I, E) :- order(E), E = orders(P, I).
