%!email : notice -> string  (oneof channel, partial)
#defined email/2.
%!A notice routed to exactly one channel — a oneof (§7.3): only the present arm shreds.
%!sort notice/1
#defined notice/1.
%!A batch of notices to send — a sequence.
%!sort outbox/1
%!notices : outbox × index -> notice  (sequence)
#defined outbox/1.
%!sms : notice -> string  (oneof channel, partial)
#defined sms/2.
%!subject : notice -> string  (total)
#defined subject/2.
