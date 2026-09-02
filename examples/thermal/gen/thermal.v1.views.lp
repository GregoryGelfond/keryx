#include "thermal.v1.core.lp".
%!alerts : alert_set × index -> alert  (sequence)
alerts(P, I, E) :- alert(E), E = alerts(P, I).
%!readings : reading_batch × index -> reading  (sequence)
readings(P, I, E) :- reading(E), E = readings(P, I).
