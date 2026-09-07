#include "signals.v1.core.lp".
%!lights : corridor × index -> light  (sequence)
lights(P, I, E) :- light(E), E = lights(P, I).
