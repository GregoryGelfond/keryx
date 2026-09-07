# The map example — a map and a cross-package field in ASP (§7.2, §4.1)

How keryx renders a Protocol Buffers **`map`** and a **cross-package** message field into
Answer Set Programming: a warehouse with a `map<string, Bin>` and a reference into another
package, and a payload `keryx facts` shreds (breaks the message down into flat, per-field
ground facts) to atoms keyed by the map key. Two things a hand-rolled shim usually flattens
or re-mints — keyed identity, and identity across a package boundary — keryx keeps.

## The schema

[`inventory.proto`](inventory.proto) imports [`catalog.proto`](catalog.proto):

```proto
// inventory.proto
syntax = "proto3";
package inventory.v1;
import "catalog.proto";

message Warehouse {
  map<string, Bin> bins = 1;   // map<string, message>
  catalog.v1.Sku featured = 2; // a field into another package
}
message Bin { uint32 quantity = 1; }
```

```proto
// catalog.proto
syntax = "proto3";
package catalog.v1;
message Sku { uint32 code = 1; }
```

## Generate the vocabulary, shred a payload

```sh
keryx gen inventory.proto -I . -o gen/     # writes both inventory.v1.* and catalog.v1.*
keryx facts --root Warehouse=warehouse.txtpb inventory.proto -I .
```

`gen` writes **one file set per package** (spec §13) — here two, in [`gen/`](gen/) — because
a schema that spans packages generates a module per package that load together.

## The map is a keyed family

`gen` writes the map as a family keyed by the map key, in
[`gen/inventory.v1.core.lp`](gen/inventory.v1.core.lp):

```prolog
%!bins : warehouse × string -> bin  (map)
```

Each entry is an **access-path term** — the value's address in the message, `bins(r0, "a-1")`
(the warehouse `r0` and the map key `"a-1"`). Shredding [`warehouse.txtpb`](warehouse.txtpb):

```prolog
bin(bins(r0, "a-1")).
bin(bins(r0, "b-2")).
quantity(bins(r0, "a-1"), 12).
quantity(bins(r0, "b-2"), 7).
```

The key `"a-1"` is part of the term itself — a rule can join on it, range over the map, or
select an entry by key. Not a synthesized row id: the key *is* the identity.

## Identity survives the package boundary

`Warehouse.featured` points into `catalog.v1`. keryx keeps the reference's identity across
the boundary: `inventory.v1.core.lp` declares

```prolog
%!featured : warehouse -> sku  (partial)
```

where `sku` is `catalog.v1`'s sort, generated in its own file set
[`gen/catalog.v1.core.lp`](gen/catalog.v1.core.lp) (which declares the sort `sku/1` and
`code : sku -> uint32 (total)`). Shredding the warehouse yields the `featured` occupant
carrying catalog's predicates:

```prolog
sku(featured(r0)).
code(featured(r0), 4090).
```

`featured(r0)` is the identity term; it *is* a `sku`, and `code` — catalog's field — applies
to it. A model over `inventory.v1` includes `catalog.v1`'s vocabulary and reasons across both
packages on the same terms, and the [manifest](gen/inventory.v1.keryx-manifest) records the
map's treatment so a change to the key or value type is a reviewable diff.

## Scope at this stage

This example demonstrates the inbound (proto→asp) direction. The asp→proto (outbound)
direction is being finalized; the generated `emit.lp` above already grounds clean under
clingo — alongside `catalog.v1`'s module, the way the two load together (the repository's
grounding gate proves it) — and this example will gain a round trip as outbound lands.
