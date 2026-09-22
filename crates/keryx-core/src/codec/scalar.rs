//! The §6 scalar policy (architecture §5, inbound): one decoded scalar [`Datum`] lowered, under
//! its field's [`ScalarTreatment`], to the themelios [`Term`] a payload fact carries — or refused
//! with the [`Diagnostic`] §6 names, at the field's path. Pure and total: a function of its
//! arguments alone, every term built through [`crate::terms`] (so each is the collapsed ground
//! leaf `terms::atom_symbol` rests on), every refusal a value. The walk applies it to each scalar a
//! payload carries — a singular field's value, a sequence's element, a map's value, and a map's
//! *key* alike (spec §7.2: keys map per §6) — and lowers enum values itself, against the referent
//! enum's mapping (§7.4).
//!
//! The one refusal §6 did not name, closed here: a `string` carrying a control character other
//! than `\n` has no spelling in themelios's clingo dialect (its string escapes are `\"`, `\\`, and
//! `\n` alone), so it is refused *before* the value is built into a fact (`UnrepresentableText`),
//! and the two delivery forms (spec §11 — the symbols and the `.lp` text) carry identical content
//! while a rendering `Unspellable` stays a genuine can't-happen. NUL, which §6 names outright,
//! keeps its own kind (`InteriorNul`).

use std::fmt::{self, Write as _};

use prost_reflect::Value;
use themelios_program::prelude::*;

use crate::codec::engine::Datum;
use crate::descriptor::model::Scalar;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Locus};
use crate::policy::model::ScalarTreatment;
use crate::terms;

/// Lower one scalar value to its term under its field's treatment, or refuse it at `at` — the
/// field's path as the walk composes it, the locus every refusal names.
///
/// | treatment | datum | term | refusal |
/// |---|---|---|---|
/// | `Native` | `I32` | the integer | — |
/// | `Native` | `U32` | the integer, when it fits `i32` | `ValueOutOfRange` above `i32::MAX` |
/// | `DecimalString` | `I64`, `U64`; `U32` under `DECIMAL_STRING` | the decimal string | — |
/// | `NativeChecked` | `I64`, `U64` | the integer, when it fits `i32` | `ValueOutOfRange` outside `[i32::MIN, i32::MAX]` |
/// | `Bool` | `Bool` | the constant `true` / `false` | — |
/// | `Text` | `Str` | the string | `InteriorNul` on a NUL; `UnrepresentableText` on any other control character but `\n` |
/// | `HexString` | `Bytes` | the lowercase-hex string | — |
/// | `NeedsAnnotation` | `F64` | — | `UnannotatedFloat`, whatever the value |
/// | `FixedPoint { n }` | `F64` | the value scaled by `10ⁿ`, rounded, when on-grid | `NonFiniteFloat`; `ValueOutOfRange` past the scaled `i32`; `ValueNotOnScale` off-grid |
/// | `OpaqueFloat` | `F64` | the shortest round-trippable decimal string | `NonFiniteFloat` |
///
/// The pairing of `treatment` and `datum` is fixed by `scalar`, the field's proto kind, from both
/// sides: the decode gives a datum the kind of its field (`engine::datum`, `engine::zero`), and
/// the policy gives a field the §6 default treatment of its kind
/// (`policy::names::scalar_treatment`) — the walk hands both here from the one
/// `ValueMapping::Scalar { kind, treatment }`. So a pairing outside the table is a keryx error,
/// never foreign input, and is discharged as one: an `unreachable` over the enumerated
/// treatments and datum kinds, checked rather than wildcarded on either axis, so a walk that
/// mis-paired a field fails loudly instead of lowering a value as another kind, and a treatment
/// or datum kind added later fails to compile here. The annotation overrides of Increment 5 widen
/// the table: `(keryx.numeric)` adds `NativeChecked` and extends `DecimalString` to a `uint32`/
/// `fixed32`; the float lowerings `(keryx.scale)`/`(keryx.opaque)` add `FixedPoint`/`OpaqueFloat`.
/// `scalar` also names the proto type in a refusal's detail — `uint32` against `fixed32`, `float`
/// against `double` — which the treatment alone cannot.
///
/// `NeedsAnnotation` reads no value: §6 makes the *field* the error, not the value, so an
/// unannotated `float`/`double` field is refused whenever the walk reaches it, its materialised
/// zero included (§5), and the detail carries §6's two-choice fix-it.
///
/// # Errors
///
/// The refusals of the table, each one `Diagnostic` at `at` — one value, one field; the walk
/// collects across fields.
pub(crate) fn lower(
    scalar: Scalar,
    treatment: ScalarTreatment,
    datum: &Datum<'_>,
    at: &str,
) -> Result<Term, Diagnostic> {
    match (treatment, datum) {
        (ScalarTreatment::Native, Datum::I32(value)) => Ok(terms::int(*value)),
        (ScalarTreatment::Native, Datum::U32(value)) => i32::try_from(*value)
            .map(terms::int)
            .map_err(|_| out_of_range(scalar, *value, at)),
        (ScalarTreatment::DecimalString, Datum::I64(value)) => Ok(decimal(value)),
        (ScalarTreatment::DecimalString, Datum::U64(value)) => Ok(decimal(value)),
        (ScalarTreatment::Bool, Datum::Bool(value)) => {
            Ok(terms::constant(if *value { "true" } else { "false" }))
        }
        (ScalarTreatment::Text, Datum::Str(value)) => text(value, at),
        (ScalarTreatment::HexString, Datum::Bytes(value)) => Ok(terms::text(&hex(value))),
        (ScalarTreatment::NeedsAnnotation, Datum::F64(_)) => Err(unannotated(scalar, at)),
        (ScalarTreatment::NativeChecked, Datum::I64(value)) => i32::try_from(*value)
            .map(terms::int)
            .map_err(|_| checked_out_of_range(scalar, value, at)),
        (ScalarTreatment::NativeChecked, Datum::U64(value)) => i32::try_from(*value)
            .map(terms::int)
            .map_err(|_| checked_out_of_range(scalar, value, at)),
        (ScalarTreatment::DecimalString, Datum::U32(value)) => Ok(decimal(value)),
        (ScalarTreatment::FixedPoint { scale }, Datum::F64(value)) => {
            lower_fixed_point(scalar, scale, *value, at)
        }
        (ScalarTreatment::OpaqueFloat, Datum::F64(value)) => lower_opaque(scalar, *value, at),
        (
            ScalarTreatment::Native
            | ScalarTreatment::DecimalString
            | ScalarTreatment::NativeChecked
            | ScalarTreatment::FixedPoint { .. }
            | ScalarTreatment::OpaqueFloat
            | ScalarTreatment::NeedsAnnotation
            | ScalarTreatment::Bool
            | ScalarTreatment::Text
            | ScalarTreatment::HexString,
            Datum::I32(_)
            | Datum::I64(_)
            | Datum::U32(_)
            | Datum::U64(_)
            | Datum::F64(_)
            | Datum::Bool(_)
            | Datum::Str(_)
            | Datum::Bytes(_)
            | Datum::Enum(_),
        ) => unreachable!(
            "the datum of a field of kind `{}` is of that kind, which its {treatment:?} treatment pairs with alone; a mis-paired lowering is a keryx error",
            scalar.as_str()
        ),
    }
}

/// The decimal-string constant of an integer (§6): its decimal text as a string term — opaque to
/// clingo's arithmetic, faithful past a native term's 32-bit integer width. The §6 default for a
/// 64-bit integer, and the `(keryx.numeric) = DECIMAL_STRING` treatment of a `uint32`/`fixed32`
/// whose top-bit value a native `i32` cannot carry (Increment 5).
fn decimal(value: impl fmt::Display) -> Term {
    terms::text(&value.to_string())
}

/// Lower a `float`/`double` under `(keryx.scale) = n` to its fixed-point native integer (§6, §26):
/// the value scaled by `10ⁿ`, rounded, admitted only when the scaled integer re-divides to the exact
/// input `f` — which for a `float` field is its exact `f64` widening, so exact re-division recovers
/// the `f32` too. Non-finite → `NonFiniteFloat`; past the scaled `i32` → `ValueOutOfRange`; off the
/// grid → `ValueNotOnScale`, which names `(keryx.opaque)` as the exact alternative. The whole
/// computation is in `f64` (the guard included), so no lossy cast is needed inbound.
// The re-division guard is an *exact* float equality by design: an epsilon margin would admit
// off-grid values and defeat the byte-for-byte property, so `clippy::float_cmp` is knowingly allowed.
#[allow(clippy::float_cmp)]
fn lower_fixed_point(scalar: Scalar, scale: u32, f: f64, at: &str) -> Result<Term, Diagnostic> {
    if !f.is_finite() {
        return Err(non_finite(scalar, at));
    }
    let pow = power_of_ten(scale);
    let Some(m) = scaled_to_i32(f * pow) else {
        return Err(scale_out_of_range(scalar, at));
    };
    // The byte-for-byte guard (§26): the scaled integer must re-divide to the exact input. `f64::from`
    // is lossless, so the comparison is exact without a narrowing cast.
    if f64::from(m) / pow != f {
        return Err(not_on_scale(scalar, at));
    }
    Ok(terms::int(m))
}

/// Lower a `float`/`double` under `(keryx.opaque) = true` to its shortest round-trippable decimal
/// string (§6): the `f32`'s decimal for a `float` field, the `f64`'s for a `double`, so the outbound
/// parse recovers the exact value. Non-finite → `NonFiniteFloat`.
fn lower_opaque(scalar: Scalar, f: f64, at: &str) -> Result<Term, Diagnostic> {
    if !f.is_finite() {
        return Err(non_finite(scalar, at));
    }
    let decimal = match scalar {
        Scalar::Float => narrow_to_f32(f).to_string(),
        Scalar::Double => f.to_string(),
        Scalar::Int32
        | Scalar::Sint32
        | Scalar::Sfixed32
        | Scalar::Uint32
        | Scalar::Fixed32
        | Scalar::Int64
        | Scalar::Uint64
        | Scalar::Fixed64
        | Scalar::Sfixed64
        | Scalar::Sint64
        | Scalar::Bool
        | Scalar::String
        | Scalar::Bytes => {
            unreachable!("`scalar_treatment` pairs `OpaqueFloat` only with a float or double kind")
        }
    };
    Ok(terms::text(&decimal))
}

/// `10ⁿ` as an exact `f64`. **Precondition: `n ≤ SCALE_MAX = 9`, not guarded here** — it is
/// discharged by the sole source of the `scale` passed in: `policy::annotate::float_treatment`
/// constructs `ScalarTreatment::FixedPoint { scale }` only after `field_treatment` has validated
/// `0 ≤ scale ≤ 9` at the policy door, and that `FixedPoint` is the only origin of the `scale`
/// reaching the two callers, [`lower_fixed_point`] and [`raise_fixed_point`]. Within the precondition
/// `10⁹ < u32::MAX` and `10ⁿ` for `n ≤ 22` is exact in `f64`, so the `u32` exponentiation cannot
/// overflow and the `From` conversion is lossless. **A new caller must re-establish `n ≤ 9`**, or
/// `10u32.pow(n)` overflows — a debug panic, a silently wrong wrapping multiply in release.
fn power_of_ten(n: u32) -> f64 {
    f64::from(10u32.pow(n))
}

/// The `i32` a finite scaled value rounds to, or `None` if its magnitude exceeds the native integer
/// width (`ValueOutOfRange`). **Cast-free:** the rounded value is a whole `f64` in range, carried
/// through its exact decimal — as [`decimal`] carries a 64-bit integer — so no lossy `as` is needed,
/// and the decimal `parse` is itself the range check that the explicit magnitude test also states.
fn scaled_to_i32(scaled: f64) -> Option<i32> {
    if scaled.abs() > f64::from(i32::MAX) {
        return None;
    }
    scaled.round().to_string().parse().ok()
}

/// Narrow an `f64` to the `f32` a `float` field carries — the one conversion the float lowerings
/// cannot avoid. A `float` value *is* an `f32`, `std` offers no `From`/`TryFrom` for the narrowing
/// (`num-traits`' checked `to_f32` would be a new dependency this increment forbids), and a decimal
/// round-trip is no substitute — it would not always reproduce the same `f32`. It is IEEE
/// round-to-nearest, exactly what the wire re-encode performs, applied only after finiteness and (for
/// fixed-point) exact re-division are checked — so nothing is lost that the `f32` field did not.
#[allow(clippy::cast_possible_truncation)]
fn narrow_to_f32(value: f64) -> f32 {
    value as f32
}

/// A `string` value as its string term, or the refusal §6 names. The kind is a function of the
/// value's content, not of its first offending character: `InteriorNul` when a NUL occurs
/// anywhere — §6's named refusal takes precedence, so a consumer keyed on it never misses a NUL
/// behind an earlier tab — else `UnrepresentableText` when any other control character but
/// `\n` does, else the term. The admission set is exactly the clingo dialect's spellable set:
/// the dialect's string rule (themelios grammar §4.4) has three escapes — `\"`, `\\`, `\n` —
/// and no other control character has a spelling, so its renderer refuses by the predicate
/// the policy refuses by, `char::is_control` less `\n`. Pinned in this module's tests:
/// character by character across the control range, and past it at the separator, format,
/// boundary, and astral code points where a printer could plausibly refuse and this one does
/// not.
fn text(value: &str, at: &str) -> Result<Term, Diagnostic> {
    if let Some(offset) = value.chars().position(|character| character == '\0') {
        return Err(interior_nul(offset, at));
    }
    if let Some((offset, character)) = value
        .chars()
        .enumerate()
        .find(|(_, character)| *character != '\n' && character.is_control())
    {
        return Err(unrepresentable(offset, character, at));
    }
    Ok(terms::text(value))
}

/// The lowercase-hex spelling of a `bytes` value (§6), two digits a byte; empty bytes spell as
/// the empty string.
fn hex(bytes: &[u8]) -> String {
    // A slice is at most `isize::MAX` long, so the doubling cannot overflow.
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// A refusal at the field's path.
fn refuse(kind: DiagnosticKind, at: &str, detail: String) -> Diagnostic {
    Diagnostic::new(kind, Locus::at(at), detail)
}

/// `ValueOutOfRange`: a `uint32`/`fixed32` value above `i32::MAX` — refused, never truncated or
/// wrapped — naming the proto type, the value, the bound, and §6's opt-out.
fn out_of_range(scalar: Scalar, value: u32, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueOutOfRange,
        at,
        format!(
            "the {} value {value} is above i32::MAX ({}), the widest integer a native clingo term carries; annotate the field `(keryx.numeric) = DECIMAL_STRING` to carry it as a decimal string",
            scalar.as_str(),
            i32::MAX
        ),
    )
}

/// `ValueOutOfRange`: a 64-bit integer under `(keryx.numeric) = NATIVE_CHECKED` that does not fit
/// clingo's native `i32` — refused, never truncated or wrapped — naming the proto type, the value
/// (a bounded integer, so it is named, as the unsigned-32 range is), the native range, and the way
/// out: drop the annotation to carry it as the §6 default decimal string. Unlike [`out_of_range`],
/// the value may be out of range below `i32::MIN` as well as above `i32::MAX`, so the message names
/// the range rather than the single upper bound.
fn checked_out_of_range(scalar: Scalar, value: impl fmt::Display, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueOutOfRange,
        at,
        format!(
            "the {} value {value} does not fit the native clingo integer range [{}, {}] that `(keryx.numeric) = NATIVE_CHECKED` requires; drop the annotation to carry it as a decimal string",
            scalar.as_str(),
            i32::MIN,
            i32::MAX
        ),
    )
}

/// `NonFiniteFloat`: a NaN or ±∞ under `(keryx.scale)`/`(keryx.opaque)` — no fixed-point integer and
/// no single decimal string round-trips it to a fixed bit pattern, so it is refused under both. Names
/// the field's type and the rule, never the value (a hostile option-driven value; P1).
fn non_finite(scalar: Scalar, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::NonFiniteFloat,
        at,
        format!(
            "a {} value is non-finite (NaN or ±∞), which no fixed-point integer or decimal string carries; §6 refuses it under both `(keryx.scale)` and `(keryx.opaque)`",
            scalar.as_str()
        ),
    )
}

/// `ValueNotOnScale`: a finite `float`/`double` off the grid its `(keryx.scale) = n` declares — the
/// scaled integer does not re-divide to the exact input, so the fixed-point lowering would not
/// round-trip (§26). Names `(keryx.opaque)` as the exact alternative, never the value.
fn not_on_scale(scalar: Scalar, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueNotOnScale,
        at,
        format!(
            "a {} value is off the grid its `(keryx.scale)` declares: its scaled integer does not re-divide to the exact value, so a fixed-point lowering would not round-trip; annotate the field `(keryx.opaque) = true` to carry it as an exact decimal string instead",
            scalar.as_str()
        ),
    )
}

/// `ValueOutOfRange`: a scaled `float`/`double` whose magnitude exceeds the native clingo integer
/// width — refused, never truncated. Names the field's type and the bound, never the value.
fn scale_out_of_range(scalar: Scalar, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueOutOfRange,
        at,
        format!(
            "a {} value scaled by its `(keryx.scale)` exceeds i32::MAX ({}), the widest integer a native clingo term carries; reduce the scale, or annotate `(keryx.opaque) = true` to carry it as a decimal string",
            scalar.as_str(),
            i32::MAX
        ),
    )
}

/// `InteriorNul`: the NUL's character offset — never the value, a payload's text being the
/// adversary's to flood a diagnostic with.
fn interior_nul(offset: usize, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::InteriorNul,
        at,
        format!(
            "the string value carries a NUL (`\\0`) at character offset {offset}, where a NUL-terminated boundary downstream would cut it short; §6 refuses the value rather than truncate it"
        ),
    )
}

/// `UnrepresentableText`: the offending character by code point and offset — never the value.
fn unrepresentable(offset: usize, character: char, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::UnrepresentableText,
        at,
        format!(
            "the string value carries the control character U+{:04X} at character offset {offset}, which has no spelling in a clingo string (the escapes are `\\\"`, `\\\\`, and `\\n` alone)",
            u32::from(character)
        ),
    )
}

/// `UnannotatedFloat`: the field, not the value, with §6's two-choice fix-it.
fn unannotated(scalar: Scalar, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::UnannotatedFloat,
        at,
        format!(
            "a {} field has no default lowering (§6); annotate it `(keryx.scale) = n` to carry a fixed-point integer (the value scaled by 10^n, range-checked) or `(keryx.opaque) = true` to carry a decimal-string constant",
            scalar.as_str()
        ),
    )
}

/// Raise one answer-set [`Symbol`] to the prost-reflect [`Value`] its field carries — the exact
/// inverse of [`lower`], under the field's `kind` and `treatment` — or refuse it at `at`, the
/// field's path (spec §12.3). The reassembly walk applies it to each scalar an occupant carries (a
/// singular value, a sequence element, a map value, a map key alike); enum values the walk raises
/// itself, against the referent enum's mapping (§7.4).
///
/// | treatment | kind | symbol | value | refusal |
/// |---|---|---|---|---|
/// | `Native` | int32/sint32/sfixed32 | `Number(n)` | `I32(n)` | another shape → `TermTypeMismatch` |
/// | `Native` | uint32/fixed32 | `Number(n)`, `n ≥ 0` | `U32(n)` | `n < 0` → `ValueOutOfRange`; another shape → `TermTypeMismatch` |
/// | `DecimalString` | int64/sint64/sfixed64 | `String(s)`, `s` an `i64` | `I64` | non-`i64` decimal → `ValueOutOfRange`; another shape → `TermTypeMismatch` |
/// | `DecimalString` | uint64/fixed64 | `String(s)`, `s` a `u64` | `U64` | non-`u64` decimal → `ValueOutOfRange`; another shape → `TermTypeMismatch` |
/// | `DecimalString` | uint32/fixed32 (`DECIMAL_STRING`) | `String(s)`, `s` a `u32` | `U32` | non-`u32` decimal → `ValueOutOfRange`; another shape → `TermTypeMismatch` |
/// | `NativeChecked` | int64/sint64/sfixed64 | `Number(n)` | `I64(n)` | another shape → `TermTypeMismatch` |
/// | `NativeChecked` | uint64/fixed64 | `Number(n)`, `n ≥ 0` | `U64(n)` | `n < 0` → `ValueOutOfRange`; another shape → `TermTypeMismatch` |
/// | `Bool` | bool | `true` / `false` constant | `Bool` | another shape → `TermTypeMismatch` |
/// | `Text` | string | `String(s)` | `String(s)` | another shape → `TermTypeMismatch` |
/// | `HexString` | bytes | `String(hex)`, even-length lowercase hex | `Bytes` | odd/non-hex or another shape → `TermTypeMismatch` |
/// | `NeedsAnnotation` | float/double | any | — | `UnannotatedFloat`, whatever the symbol |
/// | `FixedPoint { n }` | float/double | `Number(m)` | `F32`/`F64` (`m / 10ⁿ`) | another shape → `TermTypeMismatch` |
/// | `OpaqueFloat` | float/double | `String(s)`, a finite decimal | `F32`/`F64` | a non-decimal, a non-finite, or another shape → `TermTypeMismatch` |
///
/// `NeedsAnnotation` is **reachable, not `unreachable!`**: `gen` admits an unannotated `float`/
/// `double` field (the refusal is codec-time, §6/§11), so an arbitrary answer set can name it — the
/// outbound modality reaches what the inbound shred refused. A `Symbol` of the wrong shape for the
/// kind is a `TermTypeMismatch` (§12.3), never a panic (§6): the answer set is the adversary's, so a
/// mis-shaped term is refused as a value, never lowered as another kind. A refusal names the field's
/// declared type and the term's *shape*, never the term's content — the answer set is the
/// adversary's to flood a diagnostic with, as a payload's text is inbound.
///
/// # Errors
///
/// The refusals of the table, each one `Diagnostic` at `at` — one value, one field; the walk
/// collects across fields.
pub(crate) fn raise(
    value: &Symbol,
    kind: Scalar,
    treatment: ScalarTreatment,
    at: &str,
) -> Result<Value, Diagnostic> {
    match treatment {
        ScalarTreatment::Native => raise_native(value, kind, at),
        ScalarTreatment::DecimalString => raise_decimal(value, kind, at),
        ScalarTreatment::NativeChecked => raise_native_checked(value, kind, at),
        ScalarTreatment::FixedPoint { scale } => raise_fixed_point(value, kind, scale, at),
        ScalarTreatment::OpaqueFloat => raise_opaque(value, kind, at),
        ScalarTreatment::Bool => raise_bool(value, at),
        ScalarTreatment::Text => match value {
            Symbol::String(text) => Ok(Value::String(text.clone())),
            _ => Err(term_type_mismatch(kind, value, at)),
        },
        ScalarTreatment::HexString => raise_hex(value, kind, at),
        ScalarTreatment::NeedsAnnotation => Err(unannotated(kind, at)),
    }
}

/// A `Native` scalar: an integer symbol to `I32` (signed 32) or `U32` (uint32/fixed32) — the inverse
/// of `lower`'s `Native` branch, which reads an `I32` or a `U32` datum. `Symbol::Number` is an `i32`
/// (the engine's width, so a uint32 above `i32::MAX` was refused inbound at `lower`), so the only
/// range refusal here is a negative value where the kind is unsigned.
fn raise_native(value: &Symbol, kind: Scalar, at: &str) -> Result<Value, Diagnostic> {
    let Symbol::Number(number) = value else {
        return Err(term_type_mismatch(kind, value, at));
    };
    // Exhaustive over `Scalar` (no wildcard), as `lower`'s guard is: a kind added later, or one
    // `scalar_treatment` newly pairs with `Native`, fails to compile here rather than lowering
    // silently as `I32`.
    match kind {
        Scalar::Uint32 | Scalar::Fixed32 => u32::try_from(*number)
            .map(Value::U32)
            .map_err(|_| negative_unsigned(kind, *number, at)),
        Scalar::Int32 | Scalar::Sint32 | Scalar::Sfixed32 => Ok(Value::I32(*number)),
        Scalar::Int64
        | Scalar::Uint64
        | Scalar::Sint64
        | Scalar::Fixed64
        | Scalar::Sfixed64
        | Scalar::Bool
        | Scalar::Float
        | Scalar::Double
        | Scalar::String
        | Scalar::Bytes => unreachable!(
            "`scalar_treatment` pairs `Native` only with a 32-bit integer kind; a wider pairing is a keryx error"
        ),
    }
}

/// A `NativeChecked` scalar: a native integer symbol lifted to `I64` (signed 64) or `U64`
/// (uint64/fixed64) — the inverse of `lower`'s `NativeChecked` branch, which range-checked a 64-bit
/// datum into a native `i32`. `Symbol::Number` is an `i32`, in range for every signed 64-bit type;
/// the only range refusal is a negative value where the kind is unsigned, re-checked with the same
/// `negative_unsigned` the `Native` uint32 branch uses. Exhaustive over `Scalar` (no wildcard), as
/// `raise_native`'s guard is: a kind `scalar_treatment` newly pairs with `NativeChecked` fails to
/// compile here rather than lifting silently as `I64`.
fn raise_native_checked(value: &Symbol, kind: Scalar, at: &str) -> Result<Value, Diagnostic> {
    let Symbol::Number(number) = value else {
        return Err(term_type_mismatch(kind, value, at));
    };
    match kind {
        Scalar::Uint64 | Scalar::Fixed64 => u64::try_from(*number)
            .map(Value::U64)
            .map_err(|_| negative_unsigned(kind, *number, at)),
        Scalar::Int64 | Scalar::Sint64 | Scalar::Sfixed64 => Ok(Value::I64(i64::from(*number))),
        Scalar::Int32
        | Scalar::Uint32
        | Scalar::Sint32
        | Scalar::Fixed32
        | Scalar::Sfixed32
        | Scalar::Bool
        | Scalar::Float
        | Scalar::Double
        | Scalar::String
        | Scalar::Bytes => unreachable!(
            "`scalar_treatment` pairs `NativeChecked` only with a 64-bit integer kind; a narrower pairing is a keryx error"
        ),
    }
}

/// A `FixedPoint` scalar: a native integer symbol re-divided to the `float`/`double` it was scaled
/// from — the inverse of [`lower_fixed_point`]. `Symbol::Number` is the scaled integer; dividing by
/// `10ⁿ` reconstructs the value (in `f64`, `f64::from` lossless), narrowed to `f32` for a `float`
/// field (the one unavoidable cast, [`narrow_to_f32`]). Any other symbol shape is a `TermTypeMismatch`.
fn raise_fixed_point(
    value: &Symbol,
    kind: Scalar,
    scale: u32,
    at: &str,
) -> Result<Value, Diagnostic> {
    let Symbol::Number(m) = value else {
        return Err(term_type_mismatch(kind, value, at));
    };
    let back = f64::from(*m) / power_of_ten(scale);
    match kind {
        Scalar::Float => Ok(Value::F32(narrow_to_f32(back))),
        Scalar::Double => Ok(Value::F64(back)),
        Scalar::Int32
        | Scalar::Sint32
        | Scalar::Sfixed32
        | Scalar::Uint32
        | Scalar::Fixed32
        | Scalar::Int64
        | Scalar::Uint64
        | Scalar::Fixed64
        | Scalar::Sfixed64
        | Scalar::Sint64
        | Scalar::Bool
        | Scalar::String
        | Scalar::Bytes => {
            unreachable!("`scalar_treatment` pairs `FixedPoint` only with a float or double kind")
        }
    }
}

/// An `OpaqueFloat` scalar: a decimal-string symbol parsed back to the `float`/`double` it spelled —
/// the inverse of [`lower_opaque`]. A non-string symbol, a string that is not a decimal float, or one
/// that parses to a non-finite value (`inf`/`NaN`, which `parse` accepts) is a `TermTypeMismatch`:
/// the opaque treatment carries finite floats only, as the inbound lowering admits only those.
fn raise_opaque(value: &Symbol, kind: Scalar, at: &str) -> Result<Value, Diagnostic> {
    let Symbol::String(text) = value else {
        return Err(term_type_mismatch(kind, value, at));
    };
    match kind {
        Scalar::Float => match text.parse::<f32>() {
            Ok(parsed) if parsed.is_finite() => Ok(Value::F32(parsed)),
            _ => Err(term_type_mismatch(kind, value, at)),
        },
        Scalar::Double => match text.parse::<f64>() {
            Ok(parsed) if parsed.is_finite() => Ok(Value::F64(parsed)),
            _ => Err(term_type_mismatch(kind, value, at)),
        },
        Scalar::Int32
        | Scalar::Sint32
        | Scalar::Sfixed32
        | Scalar::Uint32
        | Scalar::Fixed32
        | Scalar::Int64
        | Scalar::Uint64
        | Scalar::Fixed64
        | Scalar::Sfixed64
        | Scalar::Sint64
        | Scalar::Bool
        | Scalar::String
        | Scalar::Bytes => {
            unreachable!("`scalar_treatment` pairs `OpaqueFloat` only with a float or double kind")
        }
    }
}

/// A `DecimalString` scalar: a decimal-string symbol parsed to `I64` (signed 64), `U64`
/// (uint64/fixed64), or `U32` (a `uint32`/`fixed32` under `(keryx.numeric) = DECIMAL_STRING`,
/// Increment 5) — the inverse of `lower`'s `decimal`, which spelled the integer as its decimal
/// text. The type's range *is* the parsed integer type's, so a decimal outside it (or not a decimal
/// at all) does not parse and is `ValueOutOfRange`; a non-string symbol is a shape mismatch.
fn raise_decimal(value: &Symbol, kind: Scalar, at: &str) -> Result<Value, Diagnostic> {
    let Symbol::String(text) = value else {
        return Err(term_type_mismatch(kind, value, at));
    };
    // Exhaustive over `Scalar` (no wildcard), as `lower`'s guard is: a kind newly paired with
    // `DecimalString` fails to compile here rather than parsing silently as `i64`.
    match kind {
        Scalar::Uint64 | Scalar::Fixed64 => text
            .parse::<u64>()
            .map(Value::U64)
            .map_err(|_| decimal_out_of_range(kind, at)),
        Scalar::Int64 | Scalar::Sint64 | Scalar::Sfixed64 => text
            .parse::<i64>()
            .map(Value::I64)
            .map_err(|_| decimal_out_of_range(kind, at)),
        // `(keryx.numeric) = DECIMAL_STRING` carries a `uint32`/`fixed32`'s top-bit value as a
        // decimal string (Increment 5): parse it back to `U32`, the range the parse enforces.
        Scalar::Uint32 | Scalar::Fixed32 => text
            .parse::<u32>()
            .map(Value::U32)
            .map_err(|_| decimal_out_of_range(kind, at)),
        Scalar::Int32
        | Scalar::Sint32
        | Scalar::Sfixed32
        | Scalar::Bool
        | Scalar::Float
        | Scalar::Double
        | Scalar::String
        | Scalar::Bytes => unreachable!(
            "`scalar_treatment` pairs `DecimalString` only with a 64-bit integer or a 32-bit unsigned (`uint32`/`fixed32`) kind; a signed-32 or non-integer pairing is a keryx error"
        ),
    }
}

/// A `Bool` scalar: the `true` / `false` constant to `Value::Bool` — the inverse of `lower`'s
/// `terms::constant`. A constant is a positive, zero-argument function; any other symbol (a number,
/// a string, another constant, a strongly-negated `-true`) is a shape mismatch.
fn raise_bool(value: &Symbol, at: &str) -> Result<Value, Diagnostic> {
    if let Symbol::Function {
        name,
        arguments,
        sign: Sign::Positive,
    } = value
        && arguments.is_empty()
    {
        match name.as_str() {
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            _ => {}
        }
    }
    Err(term_type_mismatch(Scalar::Bool, value, at))
}

/// A `HexString` scalar: a lowercase even-length hex string to `Value::Bytes` — the inverse of
/// `lower`'s `hex`. Any deviation (odd length, an uppercase or non-hex digit, a non-string symbol)
/// is a shape mismatch: the exact inverse admits only what keryx emits.
fn raise_hex(value: &Symbol, kind: Scalar, at: &str) -> Result<Value, Diagnostic> {
    let Symbol::String(text) = value else {
        return Err(term_type_mismatch(kind, value, at));
    };
    match unhex(text) {
        Some(bytes) => Ok(Value::Bytes(bytes.into())),
        None => Err(term_type_mismatch(kind, value, at)),
    }
}

/// Decode a lowercase, even-length hex string to bytes (the inverse of [`hex`]), or `None` on an odd
/// length or a non-lowercase-hex byte. Reads bytes, not chars: a multibyte character is not a hex
/// digit, so it is rejected by the digit check.
fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?);
    }
    Some(out)
}

/// One lowercase hex digit's value, or `None` — `[0-9a-f]` alone, as [`hex`] writes.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// The term-shape word a `TermTypeMismatch` names — the symbol's kind, never its content.
fn symbol_shape(value: &Symbol) -> &'static str {
    match value {
        Symbol::Number(_) => "a number",
        Symbol::String(_) => "a string",
        Symbol::Function { arguments, .. } if arguments.is_empty() => "a constant",
        Symbol::Function { .. } => "a compound term",
        Symbol::Tuple(_) => "a tuple",
        Symbol::Infimum | Symbol::Supremum => "a term-order bound",
    }
}

/// `TermTypeMismatch`: a term whose shape does not lower to the field's declared type (§12.3). Names
/// the type and the term's shape — never its content (the answer set is the adversary's).
fn term_type_mismatch(kind: Scalar, value: &Symbol, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::TermTypeMismatch,
        at,
        format!(
            "the answer set carries {} where the field's type `{}` is declared; the term does not lower to that type (§12.3)",
            symbol_shape(value),
            kind.as_str()
        ),
    )
}

/// `ValueOutOfRange`: a negative native integer where the kind is unsigned — refused, never wrapped
/// (the threat model's integrity property). The value is a bounded `i32`, so it is named.
fn negative_unsigned(kind: Scalar, value: i32, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueOutOfRange,
        at,
        format!(
            "the {} value {value} is negative, but {} is unsigned; the answer set is refused rather than wrapped",
            kind.as_str(),
            kind.as_str()
        ),
    )
}

/// `ValueOutOfRange`: a decimal string that is not a value the kind's integer range carries —
/// refused, never truncated. The string is not echoed (the answer set is the adversary's).
fn decimal_out_of_range(kind: Scalar, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueOutOfRange,
        at,
        format!(
            "the decimal string is not a valid {} value (a decimal integer within the type's range); refused rather than truncated",
            kind.as_str()
        ),
    )
}

#[cfg(test)]
mod tests {
    use prost_reflect::Value;
    use themelios_program::prelude::*;
    use themelios_program::render::render;

    use super::{lower, raise};
    use crate::codec::engine::Datum;
    use crate::descriptor::model::Scalar;
    use crate::diagnostics::{Diagnostic, DiagnosticKind, Locus};
    use crate::policy::model::ScalarTreatment;
    use crate::terms;

    /// The field path every case is lowered at — the locus a refusal must name.
    const AT: &str = "keryx.scalars.Sample.value";

    /// The largest `uint32` a native clingo integer carries: `i32::MAX`, as the wire's `u32`.
    fn top() -> u32 {
        u32::try_from(i32::MAX).expect("i32::MAX is a u32")
    }

    fn lowered(scalar: Scalar, treatment: ScalarTreatment, datum: &Datum<'_>) -> Term {
        lower(scalar, treatment, datum, AT).expect("admitted")
    }

    fn refused(scalar: Scalar, treatment: ScalarTreatment, datum: &Datum<'_>) -> Diagnostic {
        lower(scalar, treatment, datum, AT).expect_err("refused")
    }

    /// One fact over a term, rendered under the clingo dialect — the `.lp` spelling of a term.
    fn spelled(term: Term) -> Result<String, Unspellable> {
        render(
            &Program::of_nodes([terms::fact("p", vec![term])]),
            Dialect::Clingo,
        )
    }

    #[test]
    fn the_section_6_table_lowers_each_admitted_value_to_its_term() {
        use ScalarTreatment::{Bool, DecimalString, HexString, Native, Text};
        let table = [
            (Scalar::Int32, Native, Datum::I32(-7), terms::int(-7)),
            (
                Scalar::Sint32,
                Native,
                Datum::I32(i32::MIN),
                terms::int(i32::MIN),
            ),
            (Scalar::Uint32, Native, Datum::U32(0), terms::int(0)),
            (
                Scalar::Fixed32,
                Native,
                Datum::U32(top()),
                terms::int(i32::MAX),
            ),
            (
                Scalar::Int64,
                DecimalString,
                Datum::I64(-9_007_199_254_740_993),
                terms::text("-9007199254740993"),
            ),
            (
                Scalar::Sfixed64,
                DecimalString,
                Datum::I64(i64::MIN),
                terms::text("-9223372036854775808"),
            ),
            (
                Scalar::Uint64,
                DecimalString,
                Datum::U64(u64::MAX),
                terms::text("18446744073709551615"),
            ),
            (
                Scalar::Fixed64,
                DecimalString,
                Datum::U64(0),
                terms::text("0"),
            ),
            (
                Scalar::Bool,
                Bool,
                Datum::Bool(true),
                terms::constant("true"),
            ),
            (
                Scalar::Bool,
                Bool,
                Datum::Bool(false),
                terms::constant("false"),
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("line one\nline \"two\" \\ é 字"),
                terms::text("line one\nline \"two\" \\ é 字"),
            ),
            (Scalar::String, Text, Datum::Str(""), terms::text("")),
            (
                Scalar::Bytes,
                HexString,
                Datum::Bytes(&[0x00, 0xde, 0xad, 0xbe, 0xef, 0x0f]),
                terms::text("00deadbeef0f"),
            ),
            (Scalar::Bytes, HexString, Datum::Bytes(&[]), terms::text("")),
        ];
        for (scalar, treatment, datum, expected) in table {
            let term = lowered(scalar, treatment, &datum);
            assert_eq!(term, expected, "{scalar:?} {treatment:?} {datum:?}");
            // Every lowered term is a collapsed ground leaf — the premise `terms::atom_symbol`'s
            // discharge rests on, which holds because the policy builds only through `terms`.
            assert!(
                matches!(term, Term::Symbolic(_)),
                "{scalar:?} {treatment:?} {datum:?} lowered to an uncollapsed term"
            );
        }
    }

    #[test]
    fn the_numeric_annotation_treatments_lower_each_admitted_value() {
        use ScalarTreatment::{DecimalString, NativeChecked};
        // The Increment-5 `(keryx.numeric)` treatments, beside the §6 defaults: NATIVE_CHECKED
        // lowers a 64-bit integer to a native clingo integer within `i32` (signed and unsigned
        // alike, at the boundaries), and DECIMAL_STRING carries a `uint32`/`fixed32`'s top-bit
        // value — one a native `i32` cannot hold — as a decimal string.
        let table = [
            (
                Scalar::Int64,
                NativeChecked,
                Datum::I64(2_000_000_000),
                terms::int(2_000_000_000),
            ),
            (
                Scalar::Sfixed64,
                NativeChecked,
                Datum::I64(i64::from(i32::MIN)),
                terms::int(i32::MIN),
            ),
            (
                Scalar::Uint64,
                NativeChecked,
                Datum::U64(2_000_000_000),
                terms::int(2_000_000_000),
            ),
            (
                Scalar::Fixed64,
                NativeChecked,
                Datum::U64(u64::try_from(i32::MAX).expect("i32::MAX is a u64")),
                terms::int(i32::MAX),
            ),
            (
                Scalar::Uint32,
                DecimalString,
                Datum::U32(u32::MAX),
                terms::text("4294967295"),
            ),
            (
                Scalar::Fixed32,
                DecimalString,
                Datum::U32(2_147_483_648),
                terms::text("2147483648"),
            ),
        ];
        for (scalar, treatment, datum, expected) in table {
            let term = lowered(scalar, treatment, &datum);
            assert_eq!(term, expected, "{scalar:?} {treatment:?} {datum:?}");
            assert!(
                matches!(term, Term::Symbolic(_)),
                "{scalar:?} {treatment:?} {datum:?} lowered to an uncollapsed term"
            );
        }
    }

    #[test]
    fn the_section_6_table_refuses_each_named_case_at_the_field() {
        use DiagnosticKind::{InteriorNul, UnannotatedFloat, UnrepresentableText, ValueOutOfRange};
        use ScalarTreatment::{Native, NativeChecked, NeedsAnnotation, Text};
        let table = [
            (
                Scalar::Uint32,
                Native,
                Datum::U32(top() + 1),
                ValueOutOfRange,
            ),
            (
                Scalar::Fixed32,
                Native,
                Datum::U32(u32::MAX),
                ValueOutOfRange,
            ),
            (Scalar::String, Text, Datum::Str("a\0b"), InteriorNul),
            (
                Scalar::String,
                Text,
                Datum::Str("a\tb"),
                UnrepresentableText,
            ),
            // A carriage return, an escape sequence (a terminal-injection vector), DEL, and a C1
            // control (NEL): every control but `\n`, whichever block it comes from.
            (
                Scalar::String,
                Text,
                Datum::Str("\r\n"),
                UnrepresentableText,
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("\u{1b}[31m"),
                UnrepresentableText,
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("\u{7f}"),
                UnrepresentableText,
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("\u{85}"),
                UnrepresentableText,
            ),
            (
                Scalar::Float,
                NeedsAnnotation,
                Datum::F64(1.5),
                UnannotatedFloat,
            ),
            // The materialised zero of an unset IMPLICIT field (§5): the field is the error.
            (
                Scalar::Double,
                NeedsAnnotation,
                Datum::F64(0.0),
                UnannotatedFloat,
            ),
            // NATIVE_CHECKED on a 64-bit value outside clingo's native i32 range (Increment 5):
            // above i32::MAX, below i32::MIN, and an unsigned value above — each refused, never
            // truncated or wrapped.
            (
                Scalar::Int64,
                NativeChecked,
                Datum::I64(i64::from(i32::MAX) + 1),
                ValueOutOfRange,
            ),
            (
                Scalar::Sint64,
                NativeChecked,
                Datum::I64(i64::from(i32::MIN) - 1),
                ValueOutOfRange,
            ),
            (
                Scalar::Uint64,
                NativeChecked,
                Datum::U64(u64::try_from(i32::MAX).expect("i32::MAX is a u64") + 1),
                ValueOutOfRange,
            ),
        ];
        for (scalar, treatment, datum, kind) in table {
            let diagnostic = refused(scalar, treatment, &datum);
            assert_eq!(
                diagnostic.kind(),
                kind,
                "{scalar:?} {treatment:?} {datum:?}"
            );
            assert_eq!(
                diagnostic.locus(),
                &Locus::at(AT),
                "{scalar:?} {treatment:?} {datum:?}"
            );
        }
    }

    #[test]
    fn a_refusal_names_the_proto_type_and_its_fix_it_and_never_echoes_the_value() {
        // The range refusal names the proto type (the treatment alone cannot tell `uint32` from
        // `fixed32`), the value, the bound by its name and its width, and §6's opt-out.
        let range = refused(
            Scalar::Fixed32,
            ScalarTreatment::Native,
            &Datum::U32(u32::MAX),
        );
        let detail = range.detail();
        assert!(detail.contains("fixed32"), "{detail}");
        assert!(detail.contains("4294967295"), "{detail}");
        assert!(detail.contains("i32::MAX"), "{detail}");
        assert!(detail.contains("2147483647"), "{detail}");
        assert!(
            detail.contains("(keryx.numeric) = DECIMAL_STRING"),
            "{detail}"
        );

        // The float refusal names the proto type and carries the two-choice fix-it (§6).
        for (scalar, name) in [(Scalar::Float, "float"), (Scalar::Double, "double")] {
            let float = refused(scalar, ScalarTreatment::NeedsAnnotation, &Datum::F64(0.0));
            let detail = float.detail();
            assert!(detail.contains(name), "{detail}");
            assert!(detail.contains("(keryx.scale) = n"), "{detail}");
            assert!(detail.contains("(keryx.opaque) = true"), "{detail}");
        }

        // A text refusal locates the character by offset and names it by code point — never by
        // echoing the value, which is the payload's to flood a diagnostic with.
        let marker = "never-echoed";
        let tab = refused(
            Scalar::String,
            ScalarTreatment::Text,
            &Datum::Str(&format!("{marker}\tafter")),
        );
        let detail = tab.detail();
        assert!(!detail.contains(marker), "{detail}");
        assert!(detail.contains("U+0009"), "{detail}");
        assert!(detail.contains("offset 12"), "{detail}");
        let nul = refused(
            Scalar::String,
            ScalarTreatment::Text,
            &Datum::Str(&format!("é{marker}\0")),
        );
        let detail = nul.detail();
        assert!(!detail.contains(marker), "{detail}");
        assert!(detail.contains("offset 13"), "{detail}");
    }

    #[test]
    fn a_nul_anywhere_is_an_interior_nul_ahead_of_any_other_control() {
        // The kind is a function of the value's content, not of its first offending character:
        // §6's named refusal takes precedence, so a consumer keyed on `InteriorNul` never misses a
        // NUL behind an earlier tab.
        let behind_a_tab = refused(Scalar::String, ScalarTreatment::Text, &Datum::Str("\t\0"));
        assert_eq!(behind_a_tab.kind(), DiagnosticKind::InteriorNul);
        let alone = refused(Scalar::String, ScalarTreatment::Text, &Datum::Str("\0"));
        assert_eq!(alone.kind(), DiagnosticKind::InteriorNul);
    }

    /// Whether the policy admits the one-character string `character`, and whether the clingo
    /// dialect spells it — the two sides of the parity the admission set claims.
    fn admitted_and_spelled(character: char) -> (Result<Term, Diagnostic>, bool) {
        let value = character.to_string();
        let admitted = lower(
            Scalar::String,
            ScalarTreatment::Text,
            &Datum::Str(&value),
            AT,
        );
        let spells = spelled(terms::text(&value)).is_ok();
        (admitted, spells)
    }

    #[test]
    fn the_text_admission_set_is_the_clingo_dialect_s_spellable_set() {
        // Every C0 control, printable ASCII (the escaped `"` and `\` included), DEL, every C1
        // control, and the first character past them, each as a one-character string: the policy
        // admits it exactly when the clingo dialect spells it, so a string the policy admits never
        // meets a rendering `Unspellable` — the claim behind refusing here rather than at render,
        // pinned character by character across the whole control range (Unicode's `Cc` ends at
        // U+009F).
        for code in 0..=0xA0 {
            let character = char::from_u32(code).expect("a scalar value below the surrogates");
            let (admitted, spelled) = admitted_and_spelled(character);
            assert_eq!(
                admitted.is_ok(),
                spelled,
                "U+{code:04X}: the policy and the dialect disagree ({admitted:?})"
            );
            if let Err(diagnostic) = admitted {
                let expected = if character == '\0' {
                    DiagnosticKind::InteriorNul
                } else {
                    DiagnosticKind::UnrepresentableText
                };
                assert_eq!(diagnostic.kind(), expected, "U+{code:04X}");
            }
        }

        // Past the control range, the parity rests on the dialect's rule (themelios grammar
        // §4.4: three escapes, and no other *control* character has a spelling — the same
        // `char::is_control` the policy refuses by), and is pinned where a printer could
        // plausibly refuse and this one does not: the soft hyphen, the zero-width space, the
        // line and paragraph separators, the byte-order mark (Unicode's `Cf`/`Zl`/`Zp`, not
        // `Cc`); a private-use character; the replacement character and a noncharacter; the
        // scalars bounding the surrogate gap; and astral-plane scalars up to the last one.
        // Each is admitted by the policy *and* spelled by the dialect, rendered through
        // themelios rather than assumed.
        for code in [
            0x00AD,
            0x200B,
            0x2028,
            0x2029,
            0xD7FF,
            0xE000,
            0xFEFF,
            0xFFFD,
            0xFFFF,
            0x1F600,
            u32::from(char::MAX),
        ] {
            let character = char::from_u32(code).expect("a scalar value");
            assert!(
                !character.is_control(),
                "U+{code:04X} is not a control character"
            );
            let (admitted, spelled) = admitted_and_spelled(character);
            assert!(
                admitted.is_ok(),
                "U+{code:04X}: the policy refuses a non-control character ({admitted:?})"
            );
            assert!(
                spelled,
                "U+{code:04X}: the dialect refuses a character the policy admits"
            );
        }
    }

    #[test]
    fn the_lowered_terms_spell_as_section_6_reads() {
        // The §6 table as `.lp` text: a native integer bare, a 64-bit value and a hex string
        // quoted, the booleans as constants, a string under the dialect's three escapes.
        let arguments = vec![
            lowered(Scalar::Int32, ScalarTreatment::Native, &Datum::I32(-7)),
            lowered(Scalar::Uint32, ScalarTreatment::Native, &Datum::U32(top())),
            lowered(
                Scalar::Int64,
                ScalarTreatment::DecimalString,
                &Datum::I64(-9_007_199_254_740_993),
            ),
            lowered(Scalar::Bool, ScalarTreatment::Bool, &Datum::Bool(true)),
            lowered(
                Scalar::String,
                ScalarTreatment::Text,
                &Datum::Str("a\nb \"c\" \\"),
            ),
            lowered(
                Scalar::Bytes,
                ScalarTreatment::HexString,
                &Datum::Bytes(&[0xde, 0xad]),
            ),
        ];
        let program = Program::of_nodes([terms::fact("sample", arguments)]);
        assert_eq!(
            render(&program, Dialect::Clingo).expect("every lowered term spells"),
            "sample(-7, 2147483647, \"-9007199254740993\", true, \"a\\nb \\\"c\\\" \\\\\", \"dead\").\n"
        );
    }

    #[test]
    #[should_panic(expected = "a mis-paired lowering is a keryx error")]
    fn a_pairing_outside_the_table_is_a_keryx_error_not_a_refusal() {
        // The decode fixes a datum's kind to its field's, so a `Native` field never carries a
        // string: the discharge is checked, not wildcarded — loud, never a value lowered as
        // another kind and never a refusal that misnames a keryx bug as the payload's fault.
        let _ = lower(Scalar::Int32, ScalarTreatment::Native, &Datum::Str("x"), AT);
    }

    // --- the inverse policy: `raise`, a symbol to the value it was lowered from ---

    fn raised(kind: Scalar, treatment: ScalarTreatment, symbol: &Symbol) -> Value {
        raise(symbol, kind, treatment, AT).expect("admitted")
    }

    fn refused_raise(kind: Scalar, treatment: ScalarTreatment, symbol: &Symbol) -> Diagnostic {
        raise(symbol, kind, treatment, AT).expect_err("refused")
    }

    /// A keryx-vocabulary constant symbol (a positive, zero-argument function), as `lower` emits a
    /// bool.
    fn constant(name: &str) -> Symbol {
        Symbol::Function {
            name: Name::new(name).expect("an identifier"),
            arguments: Vec::new(),
            sign: Sign::Positive,
        }
    }

    #[test]
    fn raise_lifts_each_admitted_symbol_to_the_value_lower_produced_it_from() {
        use ScalarTreatment::{Bool, DecimalString, HexString, Native, NativeChecked, Text};
        // Signed and unsigned 32-bit natives, the 64-bit decimal strings, the booleans, a string,
        // and hex bytes — the §6 table read backwards.
        assert_eq!(
            raised(Scalar::Int32, Native, &Symbol::Number(-7)),
            Value::I32(-7)
        );
        assert_eq!(
            raised(Scalar::Sfixed32, Native, &Symbol::Number(i32::MIN)),
            Value::I32(i32::MIN)
        );
        assert_eq!(
            raised(Scalar::Uint32, Native, &Symbol::Number(0)),
            Value::U32(0)
        );
        assert_eq!(
            raised(Scalar::Fixed32, Native, &Symbol::Number(i32::MAX)),
            Value::U32(top())
        );
        assert_eq!(
            raised(
                Scalar::Int64,
                DecimalString,
                &Symbol::String("-9007199254740993".to_owned())
            ),
            Value::I64(-9_007_199_254_740_993)
        );
        assert_eq!(
            raised(
                Scalar::Uint64,
                DecimalString,
                &Symbol::String("18446744073709551615".to_owned())
            ),
            Value::U64(u64::MAX)
        );
        assert_eq!(
            raised(Scalar::Bool, Bool, &constant("true")),
            Value::Bool(true)
        );
        assert_eq!(
            raised(Scalar::Bool, Bool, &constant("false")),
            Value::Bool(false)
        );
        assert_eq!(
            raised(
                Scalar::String,
                Text,
                &Symbol::String("line \"two\" \\ é 字".to_owned())
            ),
            Value::String("line \"two\" \\ é 字".to_owned())
        );
        assert_eq!(
            raised(
                Scalar::Bytes,
                HexString,
                &Symbol::String("00deadbeef0f".to_owned())
            ),
            Value::Bytes(vec![0x00, 0xde, 0xad, 0xbe, 0xef, 0x0f].into())
        );
        assert_eq!(
            raised(Scalar::Bytes, HexString, &Symbol::String(String::new())),
            Value::Bytes(Vec::new().into())
        );
        // NATIVE_CHECKED (Increment 5): a native integer symbol lifts back to its 64-bit value —
        // signed kinds to `I64`, unsigned to `U64` (re-checked non-negative).
        assert_eq!(
            raised(Scalar::Int64, NativeChecked, &Symbol::Number(2_000_000_000)),
            Value::I64(2_000_000_000)
        );
        assert_eq!(
            raised(Scalar::Sfixed64, NativeChecked, &Symbol::Number(i32::MIN)),
            Value::I64(i64::from(i32::MIN))
        );
        assert_eq!(
            raised(
                Scalar::Uint64,
                NativeChecked,
                &Symbol::Number(2_000_000_000)
            ),
            Value::U64(2_000_000_000)
        );
        // DECIMAL_STRING on a uint32/fixed32 (Increment 5): the decimal string parses back to U32.
        assert_eq!(
            raised(
                Scalar::Uint32,
                DecimalString,
                &Symbol::String("4294967295".to_owned())
            ),
            Value::U32(u32::MAX)
        );
    }

    #[test]
    fn raise_refuses_a_mis_shaped_or_out_of_range_symbol_never_a_panic() {
        use DiagnosticKind::{TermTypeMismatch, UnannotatedFloat, ValueOutOfRange};
        use ScalarTreatment::{
            Bool, DecimalString, HexString, Native, NativeChecked, NeedsAnnotation,
        };
        let table = [
            // A negative native where the kind is unsigned — refused, never wrapped.
            (Scalar::Uint32, Native, Symbol::Number(-1), ValueOutOfRange),
            // A decimal beyond the type's range, and one that is not a decimal at all.
            (
                Scalar::Uint64,
                DecimalString,
                Symbol::String("99999999999999999999999".to_owned()),
                ValueOutOfRange,
            ),
            (
                Scalar::Int64,
                DecimalString,
                Symbol::String("not-a-number".to_owned()),
                ValueOutOfRange,
            ),
            // A wrong shape for the kind — a string where an integer is declared, a non-bool
            // constant, a number where a bool is declared.
            (
                Scalar::Int32,
                Native,
                Symbol::String("x".to_owned()),
                TermTypeMismatch,
            ),
            (Scalar::Bool, Bool, constant("maybe"), TermTypeMismatch),
            (Scalar::Bool, Bool, Symbol::Number(1), TermTypeMismatch),
            // Odd-length and non-lowercase hex — the inverse admits only what `lower` emits.
            (
                Scalar::Bytes,
                HexString,
                Symbol::String("abc".to_owned()),
                TermTypeMismatch,
            ),
            (
                Scalar::Bytes,
                HexString,
                Symbol::String("DEAD".to_owned()),
                TermTypeMismatch,
            ),
            // A float/double field is reachable outbound — a diagnostic, never a panic.
            (
                Scalar::Float,
                NeedsAnnotation,
                Symbol::Number(0),
                UnannotatedFloat,
            ),
            (
                Scalar::Double,
                NeedsAnnotation,
                Symbol::String("1.5".to_owned()),
                UnannotatedFloat,
            ),
            // NATIVE_CHECKED (Increment 5): a negative native where the kind is unsigned — refused,
            // never wrapped; a non-number symbol is a shape mismatch, never a panic.
            (
                Scalar::Uint64,
                NativeChecked,
                Symbol::Number(-1),
                ValueOutOfRange,
            ),
            (
                Scalar::Int64,
                NativeChecked,
                Symbol::String("x".to_owned()),
                TermTypeMismatch,
            ),
            // DECIMAL_STRING on a uint32 (Increment 5): a decimal beyond the u32 range — refused.
            (
                Scalar::Uint32,
                DecimalString,
                Symbol::String("4294967296".to_owned()),
                ValueOutOfRange,
            ),
        ];
        for (kind, treatment, symbol, expected) in table {
            let diagnostic = refused_raise(kind, treatment, &symbol);
            assert_eq!(diagnostic.kind(), expected, "{kind:?} {treatment:?}");
            assert_eq!(diagnostic.locus(), &Locus::at(AT), "{kind:?} {treatment:?}");
        }
    }

    #[test]
    fn a_raise_refusal_names_the_type_and_shape_never_the_answer_set_s_content() {
        // The answer set is the adversary's, so a refusal names the field's declared type and the
        // term's shape — never the term's (possibly huge, hostile) content.
        let marker = "never-echoed-secret";
        let mismatch = refused_raise(
            Scalar::Int32,
            ScalarTreatment::Native,
            &Symbol::String(marker.to_owned()),
        );
        let detail = mismatch.detail();
        assert!(!detail.contains(marker), "{detail}");
        assert!(
            detail.contains("int32") && detail.contains("a string"),
            "{detail}"
        );
        let range = refused_raise(
            Scalar::Uint64,
            ScalarTreatment::DecimalString,
            &Symbol::String(format!("{marker}9999")),
        );
        assert!(!range.detail().contains(marker), "{}", range.detail());
    }

    // --- the float treatments: (keryx.scale) fixed-point and (keryx.opaque) decimal (Increment 5) ---

    #[test]
    fn the_float_treatments_lower_each_admitted_value() {
        use ScalarTreatment::{FixedPoint, OpaqueFloat};
        // `(keryx.scale)`: a float/double on its grid scales to a native integer; `(keryx.opaque)`:
        // to its shortest round-trippable decimal string. Float and double alike.
        assert_eq!(
            lowered(Scalar::Double, FixedPoint { scale: 2 }, &Datum::F64(1.5)),
            terms::int(150)
        );
        assert_eq!(
            lowered(Scalar::Float, FixedPoint { scale: 2 }, &Datum::F64(1.5)),
            terms::int(150)
        );
        assert_eq!(
            lowered(Scalar::Double, FixedPoint { scale: 0 }, &Datum::F64(-7.0)),
            terms::int(-7)
        );
        assert_eq!(
            lowered(Scalar::Double, OpaqueFloat, &Datum::F64(1.5)),
            terms::text("1.5")
        );
        assert_eq!(
            lowered(Scalar::Float, OpaqueFloat, &Datum::F64(0.5)),
            terms::text("0.5")
        );
    }

    #[test]
    fn the_float_treatments_refuse_off_grid_non_finite_and_out_of_range() {
        use DiagnosticKind::{NonFiniteFloat, ValueNotOnScale, ValueOutOfRange};
        use ScalarTreatment::{FixedPoint, OpaqueFloat};
        // Off the declared grid: 1.234 at scale 2 rounds to 123, which re-divides to 1.23 ≠ 1.234.
        assert_eq!(
            refused(Scalar::Double, FixedPoint { scale: 2 }, &Datum::F64(1.234)).kind(),
            ValueNotOnScale
        );
        // Non-finite under either float treatment.
        assert_eq!(
            refused(
                Scalar::Double,
                FixedPoint { scale: 2 },
                &Datum::F64(f64::NAN)
            )
            .kind(),
            NonFiniteFloat
        );
        assert_eq!(
            refused(
                Scalar::Double,
                FixedPoint { scale: 2 },
                &Datum::F64(f64::INFINITY)
            )
            .kind(),
            NonFiniteFloat
        );
        assert_eq!(
            refused(Scalar::Double, OpaqueFloat, &Datum::F64(f64::NAN)).kind(),
            NonFiniteFloat
        );
        assert_eq!(
            refused(Scalar::Float, OpaqueFloat, &Datum::F64(f64::NEG_INFINITY)).kind(),
            NonFiniteFloat
        );
        // Too large for the scaled i32.
        assert_eq!(
            refused(
                Scalar::Double,
                FixedPoint { scale: 0 },
                &Datum::F64(3_000_000_000.0)
            )
            .kind(),
            ValueOutOfRange
        );
    }

    #[test]
    fn raise_lifts_the_float_treatments() {
        use ScalarTreatment::{FixedPoint, OpaqueFloat};
        // The inverse of the float lowerings: a scaled native integer re-divides to the value, a
        // decimal string parses back — a float to `F32`, a double to `F64`, bit-identical.
        assert_eq!(
            raised(
                Scalar::Double,
                FixedPoint { scale: 2 },
                &Symbol::Number(150)
            ),
            Value::F64(1.5)
        );
        assert_eq!(
            raised(Scalar::Float, FixedPoint { scale: 2 }, &Symbol::Number(150)),
            Value::F32(1.5)
        );
        assert_eq!(
            raised(
                Scalar::Double,
                OpaqueFloat,
                &Symbol::String("1.5".to_owned())
            ),
            Value::F64(1.5)
        );
        assert_eq!(
            raised(
                Scalar::Float,
                OpaqueFloat,
                &Symbol::String("0.5".to_owned())
            ),
            Value::F32(0.5)
        );
    }

    #[test]
    fn raise_refuses_a_mis_shaped_or_non_finite_float_treatment() {
        use DiagnosticKind::TermTypeMismatch;
        use ScalarTreatment::{FixedPoint, OpaqueFloat};
        // FixedPoint expects a number, opaque a parseable finite decimal. A wrong shape, a
        // non-decimal string, and a non-finite text (which `parse` would otherwise accept) are each
        // refused — opaque carries finite floats only, as the inbound lowering admits only those.
        assert_eq!(
            refused_raise(
                Scalar::Double,
                FixedPoint { scale: 2 },
                &Symbol::String("1.5".to_owned())
            )
            .kind(),
            TermTypeMismatch
        );
        assert_eq!(
            refused_raise(Scalar::Double, OpaqueFloat, &Symbol::Number(1)).kind(),
            TermTypeMismatch
        );
        assert_eq!(
            refused_raise(
                Scalar::Double,
                OpaqueFloat,
                &Symbol::String("not-a-float".to_owned())
            )
            .kind(),
            TermTypeMismatch
        );
        assert_eq!(
            refused_raise(
                Scalar::Double,
                OpaqueFloat,
                &Symbol::String("inf".to_owned())
            )
            .kind(),
            TermTypeMismatch
        );
        assert_eq!(
            refused_raise(
                Scalar::Float,
                OpaqueFloat,
                &Symbol::String("NaN".to_owned())
            )
            .kind(),
            TermTypeMismatch
        );
    }
}
