%!detail : reading -> detail  (partial)
detail(P, A) :- detail(A), A = detail(P).
