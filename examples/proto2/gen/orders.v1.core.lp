grade(a).
grade(b).
grade(unspecified).
%! A proto2 enum — a closed sort (§7.4): a number it does not declare is invalid.
%! enum grade/1  (closed)
#defined grade/1.
%! grade : order -> grade  (partial)
#defined grade/2.
%! id : order -> string  (required)
#defined id/2.
%! A ledger of orders — a sequence.
%! sort ledger/1
%! orders : ledger × index -> order  (sequence)
#defined ledger/1.
%! A purchase order — proto2 presence across the three labels (§5).
%! sort order/1
#defined order/1.
%! quantity : order -> int32  (partial)
#defined quantity/2.
%! tags : order × index -> string  (sequence)
#defined tags/3.
