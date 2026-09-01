%!An alert raised for an overheating reading.
%!sort alert/1
#defined alert/1.
%!The set of alerts emitted for a batch.
%!sort alert_set/1
#defined alert_set/1.
%!alerts : alert_set × index -> alert  (sequence)
#defined alerts/3.
%!A single sensor reading.
%!sort reading/1
#defined reading/1.
%!A batch of readings — a sequence.
%!sort reading_batch/1
#defined reading_batch/1.
%!readings : reading_batch × index -> reading  (sequence)
#defined readings/3.
%!sensor : alert -> string  (total)
%!sensor : reading -> string  (total)
#defined sensor/2.
%!temp_c : alert -> int32  (total)
%!temp_c : reading -> int32  (total)
#defined temp_c/2.
