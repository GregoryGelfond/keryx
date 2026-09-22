#include "thermal.v1.core.lp".
%! contiguity of readings : reading_batch × index -> reading  (sequence)
has_readings(P, I) :- reading(readings(P, I)).
%! totality of sensor : alert -> string  (total)
%! totality of sensor : reading -> string  (total)
has_sensor(P) :- sensor(P, _).
%! totality of temp_c : alert -> int32  (total)
%! totality of temp_c : reading -> int32  (total)
has_temp_c(P) :- temp_c(P, _).
%! alerts : alert_set -> alert  (set)
reach(A) :- alert_set(X), alerts(X, A), reach(X).
%! readings : reading_batch × index -> reading  (sequence)
reach(A) :- reach(X), reading(A), reading_batch(X), A = readings(X, I).
%! emit_alert : alert  (root)
reach(X) :- emit_alert(X).
%! emit_alert_set : alert_set  (root)
reach(X) :- emit_alert_set(X).
%! emit_reading : reading  (root)
reach(X) :- emit_reading(X).
%! emit_reading_batch : reading_batch  (root)
reach(X) :- emit_reading_batch(X).
%! functionality of sensor : alert -> string  (total)
:- alert(P), reach(P), sensor(P, V1), sensor(P, V2), V1 != V2.
%! functionality of temp_c : alert -> int32  (total)
:- alert(P), reach(P), temp_c(P, V1), temp_c(P, V2), V1 != V2.
%! totality of sensor : alert -> string  (total)
:- alert(P), reach(P), not has_sensor(P).
%! totality of temp_c : alert -> int32  (total)
:- alert(P), reach(P), not has_temp_c(P).
%! occupancy of alerts : alert_set -> alert  (set)
:- alert_set(P), alerts(P, E), reach(P), not alert(E).
%! occupancy of emit_alert : alert  (root)
:- emit_alert(P), sensor(P, _), not alert(P).
%! occupancy of emit_alert : alert  (root)
:- emit_alert(P), temp_c(P, _), not alert(P).
%! occupancy of emit_reading : reading  (root)
:- emit_reading(P), sensor(P, _), not reading(P).
%! occupancy of emit_reading : reading  (root)
:- emit_reading(P), temp_c(P, _), not reading(P).
%! occupancy of emit_reading_batch : reading_batch  (root)
:- emit_reading_batch(P), reading(readings(P, _)), not reading_batch(P).
%! contiguity of readings : reading_batch × index -> reading  (sequence)
:- has_readings(P, I), reach(P), reading_batch(P), I > 0, not has_readings(P, (I - 1)).
%! functionality of sensor : reading -> string  (total)
:- reach(P), reading(P), sensor(P, V1), sensor(P, V2), V1 != V2.
%! functionality of temp_c : reading -> int32  (total)
:- reach(P), reading(P), temp_c(P, V1), temp_c(P, V2), V1 != V2.
%! totality of sensor : reading -> string  (total)
:- reach(P), reading(P), not has_sensor(P).
%! totality of temp_c : reading -> int32  (total)
:- reach(P), reading(P), not has_temp_c(P).
%! contiguity of readings : reading_batch × index -> reading  (sequence)
:- reach(P), reading(readings(P, I)), reading_batch(P), I < 0.
%! occupancy of readings : reading_batch × index -> reading  (sequence)
:- reach(X), reading_batch(X), sensor(readings(X, I), _), not reading(readings(X, I)).
%! occupancy of readings : reading_batch × index -> reading  (sequence)
:- reach(X), reading_batch(X), temp_c(readings(X, I), _), not reading(readings(X, I)).
%! emit_alert : alert  (root)
#defined emit_alert/1.
%! emit_alert_set : alert_set  (root)
#defined emit_alert_set/1.
%! emit_reading : reading  (root)
#defined emit_reading/1.
%! emit_reading_batch : reading_batch  (root)
#defined emit_reading_batch/1.
