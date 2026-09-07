#include "dispatch.v1.core.lp".
%!notices : outbox × index -> notice  (sequence)
notices(P, I, E) :- notice(E), E = notices(P, I).
