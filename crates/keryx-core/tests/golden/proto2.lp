doc("keryx.p2.Grade", "A proto2 enum — a closed sort (§7.4).").
doc("keryx.p2.Order", "A purchase order — proto2 presence across the three labels.").
enum_t("keryx.p2.Grade", "proto2.proto", closed).
enum_value("keryx.p2.Grade", "GRADE_A", 1).
enum_value("keryx.p2.Grade", "GRADE_B", 2).
enum_value("keryx.p2.Grade", "GRADE_UNSPECIFIED", 0).
field("keryx.p2.Order", 1, "id", string, legacy_required, singular).
field("keryx.p2.Order", 2, "quantity", int32, explicit, singular).
field("keryx.p2.Order", 3, "tags", string, implicit, repeated).
file("proto2.proto", "keryx.p2").
message("keryx.p2.Order", "proto2.proto").
