//! Bounded work — allocation (§6; the threat model's bounded-work property): no count or length read
//! from the input sizes an allocation before it is checked against what the input carries. A small
//! set declaring a huge length must yield a diagnostic, not pre-allocate to the declared length (the
//! "small message, huge length" attack buys no memory).
//!
//! Measured with a counting global allocator — the one place `unsafe` is unavoidable, a test-only
//! measurement isolated to this binary, which holds a **single** test so no sibling allocations
//! pollute the peak. (Decision recorded per the plan: a test-local allocator, not a dev-dependency —
//! keryx keeps its own code and its dependency surface lean; the `#![forbid(unsafe_code)]` on each
//! library crate is untouched, this is a separate test crate relaxing the workspace `deny`.)
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// Bytes keryx may allocate per byte of *input* (a generous factor covering the schema model and the
/// decode scratch). The point is that the budget is a factor of the input length, never of a length
/// the input merely declares.
const K: usize = 4_096;
/// Fixed slack above `K · len` for one-time process/test-harness setup — well below the ~1 GiB a
/// pre-allocation to the declared length would take, so the assertion still catches one.
const BUDGET_SLACK: usize = 16 * 1024 * 1024;

#[test]
fn a_declared_huge_length_does_not_pre_allocate() {
    // Field 1 (length-delimited), a varint length claiming ~1 GiB (2^30), and no content following.
    // prost must refuse for want of bytes without first reserving the declared length.
    let set: &[u8] = &[0x0a, 0x80, 0x80, 0x80, 0x80, 0x04];
    PEAK.store(0, Ordering::SeqCst);
    let outcome = keryx_core::descriptor::ingest(set);
    let peak = PEAK.load(Ordering::SeqCst);

    assert!(outcome.is_err(), "the crafted set is refused, not ingested");
    assert!(
        peak <= K * set.len() + BUDGET_SLACK,
        "peak {peak} bytes must be bounded by the input length, not the declared ~1 GiB"
    );
}
