#include "thermal.v1.core.lp".
%! readings : reading_batch × index -> reading  (sequence)
readings(P, I, E) :- reading(E), E = readings(P, I).
