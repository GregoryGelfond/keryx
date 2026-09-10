#include "keryx.obligations.core.lp".
%! contiguity of history : gauge × index -> level  (sequence)
has_history(P, I) :- history(P, I, _).
%! totality of level : gauge -> level  (total)
has_level(P) :- level(P, _).
%! contiguity of samples : gauge × index -> int32  (sequence)
has_samples(P, I) :- samples(P, I, _).
%! totality of sensor : alarm -> string  (total)
%! totality of sensor : gauge -> string  (total)
has_sensor(P) :- sensor(P, _).
%! totality of ticks : gauge -> uint32  (total)
has_ticks(P) :- ticks(P, _).
%! membership of enum level/1  (open)
ok_level(high).
%! membership of enum level/1  (open)
ok_level(low).
%! emit_alarm : alarm  (root)
reach(X) :- emit_alarm(X).
%! emit_gauge : gauge  (root)
reach(X) :- emit_gauge(X).
%! occupancy of emit_alarm : alarm  (root)
violates("keryx.obligations.Alarm", P) :- emit_alarm(P), sensor(P, _), not alarm(P).
%! functionality of sensor : alarm -> string  (total)
violates("keryx.obligations.Alarm.sensor", P) :- alarm(P), reach(P), sensor(P, V1), sensor(P, V2), V1 != V2.
%! totality of sensor : alarm -> string  (total)
violates("keryx.obligations.Alarm.sensor", P) :- alarm(P), reach(P), not has_sensor(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- by_name(P, _, _), emit_gauge(P), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- by_tick(P, _, _), emit_gauge(P), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- channel(P, _), emit_gauge(P), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- counts(P, _, _), emit_gauge(P), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), history(P, _, _), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), level(P, _), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), manual(P, _), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), offset(P, _), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), samples(P, _, _), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), sensor(P, _), not gauge(P).
%! occupancy of emit_gauge : gauge  (root)
violates("keryx.obligations.Gauge", P) :- emit_gauge(P), ticks(P, _), not gauge(P).
%! membership of by_name : gauge × string -> level  (map)
violates("keryx.obligations.Gauge.by_name", P) :- by_name(P, K, V), gauge(P), reach(P), not ok_level(V).
%! key functionality of by_name : gauge × string -> level  (map)
violates("keryx.obligations.Gauge.by_name", P) :- by_name(P, K, V1), by_name(P, K, V2), gauge(P), reach(P), V1 != V2.
%! range of by_tick : gauge × uint32 -> uint32  (map)
violates("keryx.obligations.Gauge.by_tick", P) :- by_tick(P, K, V), gauge(P), reach(P), V < 0.
%! key functionality of by_tick : gauge × uint32 -> uint32  (map)
violates("keryx.obligations.Gauge.by_tick", P) :- by_tick(P, K, V1), by_tick(P, K, V2), gauge(P), reach(P), V1 != V2.
%! key range of by_tick : gauge × uint32 -> uint32  (map)
violates("keryx.obligations.Gauge.by_tick", P) :- by_tick(P, K, _), gauge(P), reach(P), K < 0.
%! functionality of channel : gauge -> int32  (oneof source, partial)
violates("keryx.obligations.Gauge.channel", P) :- channel(P, V1), channel(P, V2), gauge(P), reach(P), V1 != V2.
%! key functionality of counts : gauge × string -> int32  (map)
violates("keryx.obligations.Gauge.counts", P) :- counts(P, K, V1), counts(P, K, V2), gauge(P), reach(P), V1 != V2.
%! contiguity of history : gauge × index -> level  (sequence)
violates("keryx.obligations.Gauge.history", P) :- gauge(P), has_history(P, I), reach(P), I > 0, not has_history(P, (I - 1)).
%! membership of history : gauge × index -> level  (sequence)
violates("keryx.obligations.Gauge.history", P) :- gauge(P), history(P, I, V), reach(P), not ok_level(V).
%! contiguity of history : gauge × index -> level  (sequence)
violates("keryx.obligations.Gauge.history", P) :- gauge(P), history(P, I, _), reach(P), I < 0.
%! membership of level : gauge -> level  (total)
violates("keryx.obligations.Gauge.level", P) :- gauge(P), level(P, V), reach(P), not ok_level(V).
%! functionality of level : gauge -> level  (total)
violates("keryx.obligations.Gauge.level", P) :- gauge(P), level(P, V1), level(P, V2), reach(P), V1 != V2.
%! totality of level : gauge -> level  (total)
violates("keryx.obligations.Gauge.level", P) :- gauge(P), reach(P), not has_level(P).
%! functionality of manual : gauge -> string  (oneof source, partial)
violates("keryx.obligations.Gauge.manual", P) :- gauge(P), manual(P, V1), manual(P, V2), reach(P), V1 != V2.
%! functionality of offset : gauge -> int32  (partial)
violates("keryx.obligations.Gauge.offset", P) :- gauge(P), offset(P, V1), offset(P, V2), reach(P), V1 != V2.
%! contiguity of samples : gauge × index -> int32  (sequence)
violates("keryx.obligations.Gauge.samples", P) :- gauge(P), has_samples(P, I), reach(P), I > 0, not has_samples(P, (I - 1)).
%! contiguity of samples : gauge × index -> int32  (sequence)
violates("keryx.obligations.Gauge.samples", P) :- gauge(P), reach(P), samples(P, I, _), I < 0.
%! functionality of sensor : gauge -> string  (total)
violates("keryx.obligations.Gauge.sensor", P) :- gauge(P), reach(P), sensor(P, V1), sensor(P, V2), V1 != V2.
%! totality of sensor : gauge -> string  (total)
violates("keryx.obligations.Gauge.sensor", P) :- gauge(P), reach(P), not has_sensor(P).
%! exclusivity of manual | channel : gauge  (oneof source)
violates("keryx.obligations.Gauge.source", P) :- channel(P, _), gauge(P), manual(P, _), reach(P).
%! range of ticks : gauge -> uint32  (total)
violates("keryx.obligations.Gauge.ticks", P) :- gauge(P), reach(P), ticks(P, V), V < 0.
%! functionality of ticks : gauge -> uint32  (total)
violates("keryx.obligations.Gauge.ticks", P) :- gauge(P), reach(P), ticks(P, V1), ticks(P, V2), V1 != V2.
%! totality of ticks : gauge -> uint32  (total)
violates("keryx.obligations.Gauge.ticks", P) :- gauge(P), reach(P), not has_ticks(P).
%! emit_alarm : alarm  (root)
#defined emit_alarm/1.
%! emit_gauge : gauge  (root)
#defined emit_gauge/1.
