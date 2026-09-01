field("keryx.maps.Inventory", 1, "counts", map(string, int32), implicit, map).
field("keryx.maps.Inventory", 2, "items", map(int64, msg("keryx.maps.Item")), implicit, map).
field("keryx.maps.Item", 1, "sku", string, implicit, singular).
file("maps.proto", "keryx.maps").
message("keryx.maps.Inventory", "maps.proto").
message("keryx.maps.Item", "maps.proto").
