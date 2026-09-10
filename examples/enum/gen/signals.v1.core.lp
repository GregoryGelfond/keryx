%! A corridor of lights — a sequence.
%! sort corridor/1
%! lights : corridor × index -> light  (sequence)
#defined corridor/1.
%! intersection : light -> string  (total)
#defined intersection/2.
%! One intersection's current signal.
%! sort light/1
#defined light/1.
%! A traffic signal's phase — a proto3 (open) enum: §7.4 maps each value to a constant.
%! enum phase/1  (open)
#defined phase/1.
%! phase : light -> phase  (total)
#defined phase/2.
