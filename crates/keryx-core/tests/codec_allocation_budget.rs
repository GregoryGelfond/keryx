//! Bounded work — allocation at the payload door (§6; the threat model's bounded-work property):
//! no length read from a payload sizes an allocation before it is checked against what the payload
//! carries. The binary wire format declares *lengths*, never counts — a sub-message's, a packed
//! repeated field's, a string's — so the "small payload declaring a huge length" attack is the one
//! to instrument: a few bytes whose varint length claims ~1 GiB must yield `UndecodablePayload`,
//! not a reservation of the declared size.
//!
//! Measured with a counting global allocator — the one place `unsafe` is unavoidable, a test-only
//! measurement isolated to this binary, which holds a **single** test so no sibling allocations
//! pollute the peak. The descriptor door's `allocation_budget.rs` is the same instrument for its
//! door, and a separate binary for the same reason: two tests over one process-global peak would
//! pollute each other's measurement. The codec is built before anything is measured, and each
//! measured call's peak is read as growth above the bytes live when it began, so the schema's
//! resident footprint is not in the measure. The library crates keep their
//! `#![forbid(unsafe_code)]`; this is a separate test crate relaxing the workspace `deny`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use keryx_test_support as support;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::diagnostics::DiagnosticKind;

/// A pass-through allocator over the system allocator that tracks peak live bytes.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: a pass-through to `System` that only updates two atomics around the real (de)allocation; it
// returns exactly what `System` returns and never dereferences the pointer.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Bytes a shred may allocate per byte of *payload* (a generous factor covering the decode scratch
/// and the diagnostic). The point is that the budget is a factor of the payload's length, never of
/// a length the payload merely declares.
const K: usize = 4_096;
/// Fixed slack above `K · len` for the engine's per-decode setup — well below the ~1 GiB a
/// pre-allocation to the declared length would take, so the assertion still catches one.
const BUDGET_SLACK: usize = 16 * 1024 * 1024;

/// A field key followed by a varint length claiming 2^30 (~1 GiB) bytes, and no content.
const GIB: [u8; 5] = [0x80, 0x80, 0x80, 0x80, 0x04];

#[test]
fn a_declared_huge_length_does_not_pre_allocate() {
    // Built before the measurement: the codec's footprint is the baseline, not the measure.
    let thermal = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let thermal = Codec::new(&support::compile_in(
        &[thermal, support::vendored()],
        "thermal.proto",
    ))
    .expect("the thermal example builds a codec");
    let sample = Codec::new(&support::compile_fixture("scalar_treatment.proto"))
        .expect("the fixture builds a codec");

    // Three lengths a payload declares: a sub-message's (`ReadingBatch.readings`, #1), a packed
    // repeated field's (`Sample.kinds`, #10), and a string's (`Reading.sensor`, #1). The engine
    // must refuse each for want of bytes without first reserving the declared length.
    let mut sub_message = vec![0x0a];
    sub_message.extend_from_slice(&GIB);
    let mut packed = vec![0x52];
    packed.extend_from_slice(&GIB);
    let mut string = vec![0x0a];
    string.extend_from_slice(&GIB);
    let probes: [(&Codec, &str, &[u8], &str); 3] = [
        (
            &thermal,
            "ReadingBatch",
            &sub_message,
            "a sub-message length",
        ),
        (&sample, "Sample", &packed, "a packed repeated-field length"),
        (&thermal, "Reading", &string, "a string length"),
    ];
    for (codec, root_type, payload, declared) in probes {
        let baseline = LIVE.load(Ordering::SeqCst);
        PEAK.store(0, Ordering::SeqCst);
        let outcome = codec.shred(root_type, payload, PayloadFormat::Binary, &Root::fresh(0));
        let growth = PEAK.load(Ordering::SeqCst).saturating_sub(baseline);

        let diagnostics = outcome.expect_err("the crafted payload is refused, not shredded");
        assert_eq!(
            diagnostics.iter().next().expect("one diagnostic").kind(),
            DiagnosticKind::UndecodablePayload,
            "{declared}: {diagnostics}"
        );
        assert!(
            growth <= K * payload.len() + BUDGET_SLACK,
            "{declared}: the shred grew the live bytes by {growth}, which must be bounded by the payload's length, not the declared ~1 GiB"
        );
    }
}
