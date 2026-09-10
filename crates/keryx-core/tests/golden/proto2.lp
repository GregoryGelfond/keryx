doc("keryx.p2.Detail", "A message-typed occupant, so a required message field's presence obligation is exercised.").
doc("keryx.p2.Grade", "A proto2 enum — a closed sort (§7.4).").
doc("keryx.p2.Order", "A purchase order — proto2 presence across the three labels, and a required message field.").
enum_t("keryx.p2.Grade", "proto2.proto", closed).
enum_value("keryx.p2.Grade", "GRADE_A", 1).
enum_value("keryx.p2.Grade", "GRADE_B", 2).
enum_value("keryx.p2.Grade", "GRADE_UNSPECIFIED", 0).
field("keryx.p2.Detail", 1, "text", string, explicit, singular).
field("keryx.p2.Order", 1, "id", string, legacy_required, singular).
field("keryx.p2.Order", 2, "quantity", int32, explicit, singular).
field("keryx.p2.Order", 3, "tags", string, implicit, repeated).
field("keryx.p2.Order", 4, "info", msg("keryx.p2.Detail"), legacy_required, singular).
file("proto2.proto", "keryx.p2").
message("keryx.p2.Detail", "proto2.proto").
message("keryx.p2.Order", "proto2.proto").
