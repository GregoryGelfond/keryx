#include "deploy.v1.core.lp".
%! findings : report × string -> finding  (map)
findings(P, K, E) :- finding(E), E = findings(P, K).
%! services : deployment × index -> service  (sequence)
services(P, I, E) :- service(E), E = services(P, I).
