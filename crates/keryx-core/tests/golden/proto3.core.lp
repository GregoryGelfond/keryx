%!calibration : reading -> int32  (partial)
#defined calibration/2.
%!sort detail/1
#defined detail/1.
%!device : reading -> string  (oneof source, partial)
#defined device/2.
%!gateway : reading -> string  (oneof source, partial)
#defined gateway/2.
%!A proto3 enum — an open sort (§7.4).
%!enum level/1  (open)
#defined level/1.
%!level : reading -> level  (total)
#defined level/2.
%!note : detail -> string  (total)
#defined note/2.
%!sort reading/1
%!detail : reading -> detail  (partial)
#defined reading/1.
%!sensor : reading -> string  (total)
#defined sensor/2.
%!temp_c : reading -> int32  (total)
#defined temp_c/2.
