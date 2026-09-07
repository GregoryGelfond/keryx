%!code : finding -> int32  (total)
#defined code/2.
%!The configuration to validate — the inbound message (proto → facts a model reasons over).
%!sort deployment/1
%!services : deployment × index -> service  (sequence)
#defined deployment/1.
%!sort finding/1
#defined finding/1.
%!max_per_node : service -> int32  (total)
#defined max_per_node/2.
%!name : service -> string  (total)
#defined name/2.
%!reason : finding -> string  (total)
#defined reason/2.
%!replicas : service -> int32  (total)
#defined replicas/2.
%!The validation report the model computes and emits back — the outbound message
%! (answer set → proto). Keyed by service name.
%!sort report/1
%!findings : report × string -> finding  (map)
#defined report/1.
%!sort service/1
#defined service/1.
