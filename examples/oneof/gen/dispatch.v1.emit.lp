#include "dispatch.v1.core.lp".
%! contiguity of notices : outbox × index -> notice  (sequence)
has_notices(P, I) :- notice(notices(P, I)).
%! totality of subject : notice -> string  (total)
has_subject(P) :- subject(P, _).
%! notices : outbox × index -> notice  (sequence)
reach(A) :- notice(A), outbox(X), reach(X), A = notices(X, I).
%! emit_notice : notice  (root)
reach(X) :- emit_notice(X).
%! emit_outbox : outbox  (root)
reach(X) :- emit_outbox(X).
%! functionality of email : notice -> string  (oneof channel, partial)
:- email(P, V1), email(P, V2), notice(P), reach(P), V1 != V2.
%! occupancy of emit_notice : notice  (root)
:- email(P, _), emit_notice(P), not notice(P).
%! exclusivity of email | sms : notice  (oneof channel)
:- email(P, _), notice(P), reach(P), sms(P, _).
%! occupancy of notices : outbox × index -> notice  (sequence)
:- email(notices(X, I), _), outbox(X), reach(X), not notice(notices(X, I)).
%! occupancy of emit_notice : notice  (root)
:- emit_notice(P), sms(P, _), not notice(P).
%! occupancy of emit_notice : notice  (root)
:- emit_notice(P), subject(P, _), not notice(P).
%! occupancy of emit_outbox : outbox  (root)
:- emit_outbox(P), notice(notices(P, _)), not outbox(P).
%! contiguity of notices : outbox × index -> notice  (sequence)
:- has_notices(P, I), outbox(P), reach(P), I > 0, not has_notices(P, (I - 1)).
%! functionality of sms : notice -> string  (oneof channel, partial)
:- notice(P), reach(P), sms(P, V1), sms(P, V2), V1 != V2.
%! functionality of subject : notice -> string  (total)
:- notice(P), reach(P), subject(P, V1), subject(P, V2), V1 != V2.
%! totality of subject : notice -> string  (total)
:- notice(P), reach(P), not has_subject(P).
%! contiguity of notices : outbox × index -> notice  (sequence)
:- notice(notices(P, I)), outbox(P), reach(P), I < 0.
%! occupancy of notices : outbox × index -> notice  (sequence)
:- outbox(X), reach(X), sms(notices(X, I), _), not notice(notices(X, I)).
%! occupancy of notices : outbox × index -> notice  (sequence)
:- outbox(X), reach(X), subject(notices(X, I), _), not notice(notices(X, I)).
%! emit_notice : notice  (root)
#defined emit_notice/1.
%! emit_outbox : outbox  (root)
#defined emit_outbox/1.
