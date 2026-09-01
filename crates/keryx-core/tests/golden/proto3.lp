doc("keryx.p3.Level", "A proto3 enum — an open sort (§7.4).").
enum_t("keryx.p3.Level", "proto3.proto", open).
enum_value("keryx.p3.Level", "LEVEL_HIGH", 2).
enum_value("keryx.p3.Level", "LEVEL_LOW", 1).
enum_value("keryx.p3.Level", "LEVEL_UNSPECIFIED", 0).
field("keryx.p3.Detail", 1, "note", string, implicit, singular).
field("keryx.p3.Reading", 1, "sensor", string, implicit, singular).
field("keryx.p3.Reading", 2, "temp_c", int32, implicit, singular).
field("keryx.p3.Reading", 3, "calibration", int32, explicit, singular).
field("keryx.p3.Reading", 4, "level", enum("keryx.p3.Level"), implicit, singular).
field("keryx.p3.Reading", 5, "detail", msg("keryx.p3.Detail"), explicit, singular).
field("keryx.p3.Reading", 6, "device", string, explicit, singular).
field("keryx.p3.Reading", 7, "gateway", string, explicit, singular).
file("proto3.proto", "keryx.p3").
message("keryx.p3.Detail", "proto3.proto").
message("keryx.p3.Reading", "proto3.proto").
oneof("keryx.p3.Reading", "source", 6).
oneof("keryx.p3.Reading", "source", 7).
