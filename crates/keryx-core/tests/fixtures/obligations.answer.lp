% An answer set over the vocabulary of `obligations.proto` that the strict theory admits (spec
% §12.2), solved by the grounding gate (`scripts/ground.sh`): one exported gauge carrying every
% scalar-valued form an obligation reaches, each within its obligation — a value per singular
% field, a dense sequence, functional maps, one arm of the oneof, declared constants,
% non-negative unsigned values and keys; and an unexported alarm with two `sensor` values, which
% the theory must leave alone.
emit_gauge(g0).
gauge(g0).
sensor(g0, "s-1").
offset(g0, -3).
samples(g0, 0, 10).  samples(g0, 1, 20).  samples(g0, 2, 30).
counts(g0, "a", 1).  counts(g0, "b", 2).
manual(g0, "hand").
level(g0, high).
ticks(g0, 7).
history(g0, 0, low).  history(g0, 1, high).
by_name(g0, "x", low).
by_tick(g0, 3, 4).
alarm(a9).  sensor(a9, "one").  sensor(a9, "two").
