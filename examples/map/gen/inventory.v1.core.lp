%! What a bin holds.
%! sort bin/1
#defined bin/1.
%! quantity : bin -> uint32  (total)
#defined quantity/2.
%! A warehouse: a map from bin label to its contents — the key is a first-class access-path
%!  term — and a cross-package catalog reference whose identity survives the boundary.
%! sort warehouse/1
%! bins : warehouse × string -> bin  (map)
%! featured : warehouse -> sku  (partial)
#defined warehouse/1.
