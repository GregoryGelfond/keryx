% The thermal round trip (spec §28): a satisfying answer set for thermal.v1.emit.lp — the seven
% facts `keryx facts` shreds batch.binpb to, plus the root marker a model asserts to export the
% tree under r0. `keryx emit --root ReadingBatch answer.lp thermal.proto -I .` reassembles it to
% batch.reassembled.binpb — byte-for-byte batch.binpb again. A real clingo answer set also carries
% emit.lp's derived atoms (reach/1, the has_ witnesses); the reassembler keys only on the sort,
% field, and marker atoms, so they do not change the result.
emit_reading_batch(r0).
reading(readings(r0, 0)).
reading(readings(r0, 1)).
reading_batch(r0).
sensor(readings(r0, 0), "s-101").
sensor(readings(r0, 1), "s-107").
temp_c(readings(r0, 0), 44).
temp_c(readings(r0, 1), 21).
