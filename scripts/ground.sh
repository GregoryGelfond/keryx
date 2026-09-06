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
echo "ground: every theory grounds clean and solves as expected"
