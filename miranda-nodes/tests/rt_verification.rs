//! WO-3 T9 acceptance test — the real-time criteria, against a real
//! `/dev/shm` mapping and a real consumer thread.
//!
//! This is an integration test rather than a unit test on purpose: it links
//! `miranda-nodes` as an external consumer would, and it exercises the
//! file-backed shared-memory transport instead of the in-process bus the
//! dispatcher's own tests use. What it measures is the path that ships.
//!
//! Each test uses its own bus path so the suite can run in parallel without
//! two dispatchers writing the same ring.

use std::time::Duration;

use miranda_nodes::verify::{self, VerificationConfig, FRAME_BUDGET_US};

/// **The WO-3 T9 acceptance criteria.**
///
/// Frame time within budget, zero dropped frames, and No-Loop compliance on
/// the composed output — all measured on frames read back out of shared
/// memory.
///
/// Ten seconds is a compromise. It is long enough to cover roughly 600
/// frames, several respiratory cycles and a few dozen blinks, which is what
/// makes the non-repetition result meaningful. It is short enough to keep
/// `cargo test` usable. The CLI (`--bin verify-60fps`) exists for the longer
/// soak runs that a release should get.
#[test]
fn sixty_fps_acceptance_criteria_hold_on_a_real_shm_bus() {
    let report = verify::run(&VerificationConfig {
        duration: Duration::from_secs(10),
        seed: 0x5741_524E_494E_47,
        bus_path: Some("/dev/shm/miranda_t9_acceptance".into()),
        cleanup: true,
    })
    .expect("verification harness failed to start");

    print!("{}", report.render());

    let failures = report.failures();
    assert!(
        failures.is_empty(),
        "WO-3 T9 criteria not met:\n{}",
        failures
            .iter()
            .map(|f| format!("  - {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // Restate the headline criteria explicitly. `failures()` already covers
    // them, but a reader of this file should be able to see the actual
    // numbers being asserted rather than having to trust a helper.
    assert!(
        report.max_build_us <= FRAME_BUDGET_US,
        "worst frame build {:.3} us over the {FRAME_BUDGET_US} us budget",
        report.max_build_us
    );
    assert_eq!(report.frames_dropped, 0, "dropped frames must be zero");
    assert_eq!(
        report.identical_consecutive_frames, 0,
        "consecutive frames must never be identical"
    );
    assert_eq!(
        report.repeated_frames, 0,
        "no frame may exactly repeat an earlier one"
    );
    assert_eq!(report.out_of_range_values, 0);
    assert!(
        report.frames_consumed > 0,
        "nothing was read back from shared memory"
    );
}

/// The velocity clamp must still hold on frames that made the round trip
/// through shared memory, not only on the damper's own output. A transport
/// bug that dropped or reordered frames would show up here as a per-frame
/// channel delta above the cap, because the observed jump would span two
/// frame periods.
#[test]
fn velocity_clamp_holds_on_frames_read_back_from_shared_memory() {
    let report = verify::run(&VerificationConfig {
        duration: Duration::from_secs(6),
        seed: 0x1234_5678_9ABC_DEF0,
        bus_path: Some("/dev/shm/miranda_t9_velocity".into()),
        cleanup: true,
    })
    .expect("verification harness failed to start");

    println!(
        "max channel delta observed on the bus: {:.6} (cap {:.6}) over {} frames",
        report.max_channel_delta, report.channel_delta_cap, report.frames_consumed
    );

    assert!(report.frames_consumed > 100, "run too short to be meaningful");
    assert!(
        report.max_channel_delta <= report.channel_delta_cap + 1e-4,
        "a channel moved {} in one frame on the bus, over the {} cap",
        report.max_channel_delta,
        report.channel_delta_cap
    );
    // A cap that is never approached would mean the test is not exercising
    // the clamp at all. Idle autonomic motion is small, so this only checks
    // that motion is happening, not that it saturates.
    assert!(
        report.max_channel_delta > 0.0,
        "no channel moved at all between frames"
    );
}

/// The observed rate on the bus must be close to 60 Hz, and the inter-frame
/// gaps must cluster near the nominal period.
///
/// The band is wide deliberately. This runs on a general-purpose desktop
/// kernel, on a dual-core Celeron, with no real-time priority, so a tight
/// bound here would be a flaky test rather than a meaningful one. What it
/// actually catches is a pacer that is wrong by a factor — no sleep at all,
/// or a doubled period — not a few percent of scheduler noise.
#[test]
fn observed_bus_cadence_is_close_to_sixty_hertz() {
    let report = verify::run(&VerificationConfig {
        duration: Duration::from_secs(6),
        seed: 99,
        bus_path: Some("/dev/shm/miranda_t9_cadence".into()),
        cleanup: true,
    })
    .expect("verification harness failed to start");

    println!(
        "observed {:.2} fps; gaps min {} us, mean {:.1} us, max {} us; \
         {} gaps implying a drop",
        report.observed_fps,
        report.min_gap_us,
        report.mean_gap_us,
        report.max_gap_us,
        report.gaps_implying_a_drop
    );

    assert!(
        (45.0..=62.0).contains(&report.observed_fps),
        "observed {:.2} fps on the bus, expected roughly 60",
        report.observed_fps
    );
    assert!(
        (14_000.0..=19_000.0).contains(&report.mean_gap_us),
        "mean inter-frame gap {:.1} us, expected near 16667",
        report.mean_gap_us
    );
    assert_eq!(
        report.gaps_implying_a_drop, 0,
        "{} timestamp gaps imply missing frames",
        report.gaps_implying_a_drop
    );
    assert_eq!(report.non_monotonic_timestamps, 0);
}
