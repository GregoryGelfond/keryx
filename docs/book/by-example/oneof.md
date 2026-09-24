# A oneof in ASP

How keryx renders a Protocol Buffers **`oneof`** into Answer Set Programming: a message with a two-armed oneof, and a payload `keryx facts` shreds so that **only the arm actually set** appears. Presence is exactly where a hand-rolled shim tends to go wrong.

The runnable example is in [`examples/oneof/`](https://github.com/GregoryGelfond/keryx/tree/main/examples/oneof).

## The schema

```proto
syntax = "proto3";
package dispatch.v1;

message Notice {
  string subject = 1;
  oneof channel {
    string email = 2;
    string sms = 3;
  }
}
message Outbox { repeated Notice notices = 1; }
```

## The arms are partial functions

`gen` writes each arm of the oneof as a **partial** function, tagged with the oneof it belongs to:

```prolog
%! email : notice -> string  (oneof channel, partial)
#defined email/2.
%! sms : notice -> string  (oneof channel, partial)
#defined sms/2.
%! subject : notice -> string  (total)
#defined subject/2.
```

`subject` is total (always present); `email` and `sms` are partial — each may or may not hold, and both belong to the oneof `channel`.

## Only the present arm shreds

A payload with two notices — the first sets `email`, the second `sms` — shreds to:

```prolog
email(notices(r0, 0), "ops@example.com").
sms(notices(r0, 1), "+15551234").
subject(notices(r0, 0), "Deploy complete").
subject(notices(r0, 1), "Disk 90% full").
```

Notice 0 has an `email` atom and **no** `sms` atom; notice 1 the reverse. Absence is honest absence — not a null, not an empty string, not both arms. A rule that joins on `email` sees exactly the notices that have one.

## The exclusivity the theory keeps

Outbound, `emit.lp` holds a notice to **at most one** arm of the oneof:

```prolog
%! exclusivity of email | sms : notice  (oneof channel)
:- email(P, _), notice(P), reach(P), sms(P, _).
```

An answer set that set both arms of a `channel` is simply not a message — UNSAT under the strict theory. The manifest records each arm as a member of the oneof, so a rearranged or renamed arm is a reviewable diff.
