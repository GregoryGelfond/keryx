#!/usr/bin/env bash
#
# The grounding gate: the generated serializability theory read the way a solver reads it.
#
# The goldens compare `emit.lp` text to text, so a rule the grounder rejects (an unsafe
# variable) or one that fires against the wrong sort would pass them unnoticed. This gate
# generates both theories for three schemas with `keryx gen --shape both`, grounds them, and
# solves a small answer set under them:
#
#   thermal      examples/thermal/thermal.proto — the worked example: a sequence of messages,
#                and the `sensor`/`temp_c` predicates two sorts share.
#   reach        crates/keryx-core/tests/fixtures/reach.proto — one parent sort holding a
#                message in every form (a singular field, a sequence, a map, a oneof arm), so
#                the map- and singular-form closure rules are grounded, not the sequence form
#                alone. The three forms share one safety — the child sort atom binds the
#                occupant before the equality deconstructs it — and the grounder confirms each.
#   obligations  crates/keryx-core/tests/fixtures/obligations.proto — every scalar-valued
#                obligation on one sort.
#   crossmap     crates/keryx-core/tests/fixtures/gmap.proto (+ gdep.proto) — a map<uint32,
#                message> whose key range reads the occupant's sort atom, not a field atom, and a
#                cross-package message field whose child sort lives in the other package's unit (so
#                this unit emits the reach step and root occupancy for it but no slot occupancy).
#                Grounded together with the dependency package's core.lp, the way the files load.
#
# For each schema the gate asserts: the strict and the diagnostic theory ground clean (no
# error, no undefined-atom notice); a serializable answer set is SAT under the strict theory;
# the same answer set with a second value for a singular field is UNSAT under the strict
# theory and, under the diagnostic one, SAT with the violation derived at the field's path.
# Thermal's answer set is its own shredded facts under an export marker: the wire's theorems
# are the outbound obligations, so a shredded payload satisfies the theory it is reassembled
# under.
#
# Test infrastructure, never keryx: the gate drives the `clingo` on PATH; keryx links and
# spawns no solver. CI installs clingo and runs this job; locally, clingo 5.x must be on PATH.
#
# Usage: scripts/ground.sh   (from anywhere; builds keryx unless KERYX names a binary)
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
fixtures=$root/crates/keryx-core/tests/fixtures
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
  echo "ground: $*" >&2
  exit 1
}

command -v clingo > /dev/null || fail "clingo is not on PATH"
if [ -z "${KERYX:-}" ]; then
  (cd "$root" && cargo build --locked --quiet --bin keryx)
  KERYX=${CARGO_TARGET_DIR:-$root/target}/debug/keryx
fi

# ground FILE — the theory grounds clean: clingo accepts it with nothing on stderr. The
# grounder's exit is non-zero on an error (an unsafe variable); an undefined-atom notice is a
# zero exit with text, so the text is the check.
ground() {
  local file=$1 notices
  notices=$(clingo --text --warn=all "$file" 2>&1 > /dev/null) \
    || fail "$(basename "$file") does not ground:
$notices"
  [ -z "$notices" ] || fail "$(basename "$file") grounds with notices:
$notices"
  echo "ground: $(basename "$file") grounds clean"
}

# ground_with FILE EXTRA... — as `ground`, but FILE grounds alongside EXTRA (a dependency package's
# core.lp), so a cross-package sort atom resolves rather than being reported undefined — the way the
# two packages' files load together. Grounded alone, that atom is undefined by design.
ground_with() {
  local file=$1 notices
  shift
  notices=$(clingo --text --warn=all "$file" "$@" 2>&1 > /dev/null) \
    || fail "$(basename "$file") does not ground:
$notices"
  [ -z "$notices" ] || fail "$(basename "$file") grounds with notices:
$notices"
  echo "ground: $(basename "$file") grounds clean"
}

# solve VERDICT FILE... — clingo's verdict on FILE... is VERDICT (`sat` or `unsat`), by its
# exit status: 10 or 30 satisfiable, 20 unsatisfiable. The model's text is left in $model.
solve() {
  local verdict=$1 status=0
  shift
  model=$(clingo --warn=all "$@" 2> /dev/null) || status=$?
  case "$verdict:$status" in
    sat:10 | sat:30 | unsat:20) ;;
    *) fail "expected $verdict from clingo $*, got exit $status" ;;
  esac
}

# grounds_clean NAME PROTO INCLUDE PACKAGE [DEP...] — generate PACKAGE's strict and diagnostic
# theories from PROTO and assert both ground clean. For a feature example (examples/<name>) whose
# value is the inbound shred: the generated outbound theory is proven sound — safe, no undefined
# atom — without an answer set to solve. Each DEP (a dependency package generated beside PACKAGE)
# grounds alongside as its core.lp, so a cross-package sort atom resolves, as `ground_with` does
# for the cross-package holder.
grounds_clean() {
  local name=$1 proto=$2 include=$3 package=$4
  shift 4
  local dir=$work/$name
  mkdir -p "$dir"
  "$KERYX" gen "$proto" -I "$include" -o "$dir" --shape both 2> "$dir/gen.log" \
    || fail "keryx gen $proto failed:
$(cat "$dir/gen.log")"
  local deps=() dep
  for dep in "$@"; do deps+=("$dir/$dep.core.lp"); done
  if [ "${#deps[@]}" -eq 0 ]; then
    ground "$dir/$package.emit.lp"
    ground "$dir/$package.emit-diagnostic.lp"
  else
    ground_with "$dir/$package.emit.lp" "${deps[@]}"
    ground_with "$dir/$package.emit-diagnostic.lp" "${deps[@]}"
  fi
  echo "ground: $name — strict and diagnostic theories ground clean"
}

# check NAME PROTO INCLUDE PACKAGE ANSWER DUPLICATE VIOLATION — generate PACKAGE's theories
# from PROTO, ground both, and solve ANSWER: SAT under strict; with the DUPLICATE atom added,
# UNSAT under strict and SAT under diagnostic with VIOLATION derived.
check() {
  local name=$1 proto=$2 include=$3 package=$4 answer=$5 duplicate=$6 violation=$7
  local dir=$work/$name
  mkdir -p "$dir"
  "$KERYX" gen "$proto" -I "$include" -o "$dir" --shape both 2> "$dir/gen.log" \
    || fail "keryx gen $proto failed:
$(cat "$dir/gen.log")"
  local strict=$dir/$package.emit.lp diagnostic=$dir/$package.emit-diagnostic.lp
  ground "$strict"
  ground "$diagnostic"
  printf '%s\n' "$duplicate" > "$dir/duplicate.lp"
  solve sat "$strict" "$answer"
  solve unsat "$strict" "$answer" "$dir/duplicate.lp"
  solve sat "$diagnostic" "$answer" "$dir/duplicate.lp"
  case "$model" in
    *"$violation"*) ;;
    *) fail "the diagnostic theory did not derive $violation:
$model" ;;
  esac
  echo "ground: $name — SAT; UNSAT on a duplicate; $violation derived"
}

# Thermal's answer set: the committed facts of the committed payload, exported as they were
# shredded.
{
  cat "$root/examples/thermal/gen/thermal.v1.facts.lp"
  echo 'emit_reading_batch(r0).'
} > "$work/thermal.answer.lp"

check thermal "$root/examples/thermal/thermal.proto" "$root/examples/thermal" thermal.v1 \
  "$work/thermal.answer.lp" \
  'sensor(readings(r0, 0), "s-999").' \
  'violates("thermal.v1.Reading.sensor",readings(r0,0))'
check reach "$fixtures/reach.proto" "$fixtures" keryx.reach \
  "$fixtures/reach.answer.lp" \
  'note(first(p0), "again").' \
  'violates("keryx.reach.Step.note",first(p0))'
check obligations "$fixtures/obligations.proto" "$fixtures" keryx.obligations \
  "$fixtures/obligations.answer.lp" \
  'sensor(g0, "s-2").' \
  'violates("keryx.obligations.Gauge.sensor",g0)'

# crossmap — grounded with the dependency package's core.lp (see the header): the holder theory's
# map<uint32,message> key range and its cross-package reach step and root occupancy all ground
# clean; a valid holder is SAT; a negative map key is UNSAT under strict and, under diagnostic,
# SAT with the key-range violation derived at the field's path.
crossmap=$work/crossmap
mkdir -p "$crossmap"
"$KERYX" gen "$fixtures/gmap.proto" -I "$fixtures" -o "$crossmap" --shape both 2> "$crossmap/gen.log" \
  || fail "keryx gen gmap.proto failed:
$(cat "$crossmap/gen.log")"
gmap_strict=$crossmap/keryx.gmap.emit.lp
gmap_diagnostic=$crossmap/keryx.gmap.emit-diagnostic.lp
gdep_core=$crossmap/keryx.gdep.core.lp
ground_with "$gmap_strict" "$gdep_core"
ground_with "$gmap_diagnostic" "$gdep_core"
printf '%s\n' 'emit_holder(h0).' 'holder(h0).' 'inner(by_id(h0, 0)).' 'n(by_id(h0, 0), 5).' \
  > "$crossmap/answer.lp"
printf '%s\n' 'inner(by_id(h0, -1)).' 'n(by_id(h0, -1), 7).' > "$crossmap/badkey.lp"
solve sat "$gmap_strict" "$gdep_core" "$crossmap/answer.lp"
solve unsat "$gmap_strict" "$gdep_core" "$crossmap/answer.lp" "$crossmap/badkey.lp"
solve sat "$gmap_diagnostic" "$gdep_core" "$crossmap/answer.lp" "$crossmap/badkey.lp"
case "$model" in
  *'violates("keryx.gmap.Holder.by_id",h0)'*) ;;
  *) fail "the diagnostic theory did not derive the key-range violation:
$model" ;;
esac
echo "ground: crossmap — SAT; UNSAT on a negative map key; key-range violation derived"

# The feature examples (examples/<name>) — inbound-focused, so the gate is that the generated
# outbound theory grounds clean, not an answer-set solve.
grounds_clean enum "$root/examples/enum/signals.proto" "$root/examples/enum" signals.v1
grounds_clean oneof "$root/examples/oneof/dispatch.proto" "$root/examples/oneof" dispatch.v1
grounds_clean map "$root/examples/map/inventory.proto" "$root/examples/map" inventory.v1 catalog.v1
# proto2 (E1, #1): a proto2 `required` field carries an outbound totality obligation, so the
# generated theory both grounds clean and enforces completeness. An `Order` naming its `required`
# id is SAT under strict; one omitting id is UNSAT under strict and, under diagnostic, SAT with the
# id totality violation derived at the field's path — full proto2/proto3 outbound parity.
proto2=$work/proto2
mkdir -p "$proto2"
"$KERYX" gen "$root/examples/proto2/order.proto" -I "$root/examples/proto2" -o "$proto2" --shape both \
  2> "$proto2/gen.log" || fail "keryx gen order.proto failed:
$(cat "$proto2/gen.log")"
ground "$proto2/orders.v1.emit.lp"
ground "$proto2/orders.v1.emit-diagnostic.lp"
printf '%s\n' 'emit_order(o0).' 'order(o0).' 'id(o0, "PO-1").' > "$proto2/answer.lp"
printf '%s\n' 'emit_order(o0).' 'order(o0).' > "$proto2/missing.lp"
solve sat "$proto2/orders.v1.emit.lp" "$proto2/answer.lp"
solve unsat "$proto2/orders.v1.emit.lp" "$proto2/missing.lp"
solve sat "$proto2/orders.v1.emit-diagnostic.lp" "$proto2/missing.lp"
case "$model" in
  *'violates("orders.v1.Order.id",o0)'*) ;;
  *) fail "the proto2 diagnostic theory did not derive the required-field violation:
$model" ;;
esac
echo "ground: proto2 — SAT with the required id; UNSAT omitting it; required-field violation derived"

# config — the computed example (examples/config): the generated theory grounds clean, and the
# consuming tool's model.lp, over the shredded config, derives a Report that satisfies the theory.
# clingo here is test infrastructure driving the PATH solver — keryx itself spawns none.
config=$work/config
mkdir -p "$config"
"$KERYX" gen "$root/examples/config/deployment.proto" -I "$root/examples/config" -o "$config" --shape both \
  2> "$config/gen.log" || fail "keryx gen deployment.proto failed:
$(cat "$config/gen.log")"
ground "$config/deploy.v1.emit.lp"
ground "$config/deploy.v1.emit-diagnostic.lp"
# The bad config: the model derives exactly the finding set answer.bad.lp claims (web and cache,
# no others), and the Report satisfies the strict theory.
solve sat "$config/deploy.v1.emit.lp" "$root/examples/config/facts.bad.lp" "$root/examples/config/model.lp"
for want in 'finding(findings(v0,"web"))' 'finding(findings(v0,"cache"))'; do
  case "$model" in
    *"$want"*) ;;
    *) fail "config: the bad config did not derive $want:
$model" ;;
  esac
done
case "$model" in
  *'finding(findings(v0,"api"))'*) fail "config: the bad config derived a spurious api finding:
$model" ;;
  *) ;;
esac
# The valid config: the model derives an empty Report — no findings.
solve sat "$config/deploy.v1.emit.lp" "$root/examples/config/facts.ok.lp" "$root/examples/config/model.lp"
case "$model" in
  *'finding(findings('*) fail "config: the valid config derived a finding:
$model" ;;
  *) ;;
esac
echo "ground: config — theory grounds clean; bad config derives findings; valid config derives none"

echo "ground: every theory grounds clean and solves as expected"
