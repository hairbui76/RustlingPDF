//! Launch-intent parsing and multi-select aggregation for Explorer context
//! menu launches.
//!
//! The Windows MSI registers a cascade context menu whose verbs invoke
//! `RustlingPDF.exe --tool <action> "%1"`. Explorer launches one process per
//! selected file, so an N-file multi-select becomes N single-instance
//! callbacks in the primary process. This module turns those N callbacks into
//! ONE frontend batch: launches carrying the same intent are buffered and
//! flushed after a sliding debounce window (bounded by a hard cap).
//!
//! `TOOL_INTENT_ACTIONS` is one leg of a single-source-of-truth triple that
//! must stay identical across:
//! - this allowlist,
//! - `windows/wix/provisioning.wxs` (the `--tool <action>` verb commands),
//! - `src/desktop/services/toolIntentService.ts` (`TOOL_INTENT_ACTIONS`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

/// Actions the `--tool` flag accepts. Anything else degrades to a plain open.
pub const TOOL_INTENT_ACTIONS: [&str; 4] = ["open", "merge", "compress", "convert"];

/// Sliding debounce window: a batch flushes once no new launch with the same
/// intent has arrived for this long.
pub const INTENT_DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);

/// Hard cap: a batch never waits longer than this from its first launch, even
/// while launches keep trickling in (worst case it flushes within one debounce
/// window past the cap).
pub const INTENT_MAX_AGGREGATION: Duration = Duration::from_secs(3);

/// Result of parsing a launch's command-line arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLaunch {
    /// Existing file paths, in argument order.
    pub files: Vec<String>,
    /// Validated tool intent, if a known `--tool <action>` was passed.
    pub tool: Option<String>,
}

/// Parse CLI args (skipping the executable name) into file paths plus an
/// optional tool intent.
///
/// Grammar and degradation rules:
/// - `--tool <name>` may appear anywhere between paths; the value is consumed
///   and never treated as a path. The last occurrence wins.
/// - A `<name>` outside [`TOOL_INTENT_ACTIONS`], or a trailing `--tool`
///   without a value, degrades the launch to a plain open (`tool: None`).
/// - Every other argument is kept iff it names an existing path — bare-path
///   launches (double-click, drag-drop relaunch, macOS/Linux) parse exactly
///   as they did before the flag existed.
pub fn parse_launch_args(args: &[String]) -> ParsedLaunch {
    parse_launch_args_impl(args, |candidate| {
        std::path::Path::new(candidate).exists()
    })
}

fn parse_launch_args_impl<F: Fn(&str) -> bool>(args: &[String], path_exists: F) -> ParsedLaunch {
    let mut files = Vec::new();
    let mut tool: Option<String> = None;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--tool" {
            tool = iter.next().and_then(|name| {
                TOOL_INTENT_ACTIONS
                    .contains(&name.as_str())
                    .then(|| name.clone())
            });
        } else if path_exists(arg) {
            files.push(arg.clone());
        }
    }
    ParsedLaunch { files, tool }
}

struct PendingBatch {
    files: Vec<String>,
    first_at: Instant,
    generation: u64,
}

/// Per-intent buffer of launches waiting for their debounce window to close.
///
/// Every `add` returns a generation token and is expected to be paired with
/// one sleeper that later calls `take_if_quiescent` with that token; exactly
/// one sleeper flushes any given batch.
pub struct IntentAggregator {
    pending: Mutex<Option<HashMap<String, PendingBatch>>>,
    // Monotonic across ALL batches so a sleeper from an already-flushed batch
    // can never observe a colliding generation on a newer batch of the same
    // intent (which would flush it prematurely).
    next_generation: AtomicU64,
}

impl IntentAggregator {
    pub const fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            next_generation: AtomicU64::new(0),
        }
    }

    /// Buffer `files` under `tool`, returning the generation token the paired
    /// sleeper must present to flush.
    pub fn add(&self, tool: &str, files: Vec<String>, now: Instant) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let mut guard = self.pending.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        let entry = map
            .entry(tool.to_string())
            .or_insert_with(|| PendingBatch {
                files: Vec::new(),
                first_at: now,
                generation,
            });
        entry.files.extend(files);
        entry.generation = generation;
        generation
    }

    /// Flush policy, evaluated when a sleeper wakes: take the batch when no
    /// newer launch superseded this sleeper (sliding debounce) or when the
    /// batch has been pending at least [`INTENT_MAX_AGGREGATION`] (hard cap).
    pub fn take_if_quiescent(
        &self,
        tool: &str,
        observed_generation: u64,
        now: Instant,
    ) -> Option<Vec<String>> {
        let mut guard = self.pending.lock().unwrap();
        let map = guard.as_mut()?;
        let entry = map.get(tool)?;
        let quiescent = entry.generation == observed_generation;
        let capped = now.duration_since(entry.first_at) >= INTENT_MAX_AGGREGATION;
        if quiescent || capped {
            map.remove(tool).map(|batch| batch.files)
        } else {
            None
        }
    }
}

impl Default for IntentAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide aggregator shared by the cold-launch path and the
/// single-instance callback.
pub static INTENT_AGGREGATOR: IntentAggregator = IntentAggregator::new();

/// Debounce driver: sleep one window, then flush if this sleeper is still the
/// newest for its intent (or the hard cap expired). Returns the aggregated
/// files when this sleeper is the one that should forward them.
pub async fn debounce_intent_batch(
    aggregator: &IntentAggregator,
    tool: &str,
    observed_generation: u64,
) -> Option<Vec<String>> {
    tokio::time::sleep(INTENT_DEBOUNCE_WINDOW).await;
    aggregator.take_if_quiescent(tool, observed_generation, Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // ── parsing ────────────────────────────────────────────────────────────

    #[test]
    fn allowlist_is_the_canonical_action_set() {
        assert_eq!(
            TOOL_INTENT_ACTIONS,
            ["open", "merge", "compress", "convert"]
        );
    }

    #[test]
    fn bare_paths_parse_byte_compatibly() {
        let parsed = parse_launch_args_impl(&args(&["exe", "/a.pdf", "/b.pdf"]), |p| {
            p != "/missing.pdf"
        });
        assert_eq!(parsed.files, vec!["/a.pdf", "/b.pdf"]);
        assert_eq!(parsed.tool, None);
    }

    #[test]
    fn nonexistent_paths_are_dropped() {
        let parsed = parse_launch_args_impl(&args(&["exe", "/a.pdf", "/missing.pdf"]), |p| {
            p == "/a.pdf"
        });
        assert_eq!(parsed.files, vec!["/a.pdf"]);
        assert_eq!(parsed.tool, None);
    }

    #[test]
    fn tool_flag_parses_with_interleaved_paths() {
        let parsed = parse_launch_args_impl(
            &args(&["exe", "/a.pdf", "--tool", "merge", "/b.pdf"]),
            |p| p.ends_with(".pdf"),
        );
        assert_eq!(parsed.files, vec!["/a.pdf", "/b.pdf"]);
        assert_eq!(parsed.tool, Some("merge".to_string()));
    }

    #[test]
    fn tool_value_is_never_treated_as_a_path() {
        // Even a path-exists check that would match the value must not turn
        // the consumed flag value into an opened file.
        let parsed = parse_launch_args_impl(&args(&["exe", "--tool", "merge"]), |_| true);
        assert_eq!(parsed.files, Vec::<String>::new());
        assert_eq!(parsed.tool, Some("merge".to_string()));
    }

    #[test]
    fn unknown_tool_degrades_to_plain_open() {
        let parsed = parse_launch_args_impl(
            &args(&["exe", "--tool", "frobnicate", "/a.pdf"]),
            |p| p == "/a.pdf",
        );
        assert_eq!(parsed.files, vec!["/a.pdf"]);
        assert_eq!(parsed.tool, None);
    }

    #[test]
    fn trailing_tool_flag_without_value_degrades() {
        let parsed =
            parse_launch_args_impl(&args(&["exe", "/a.pdf", "--tool"]), |p| p == "/a.pdf");
        assert_eq!(parsed.files, vec!["/a.pdf"]);
        assert_eq!(parsed.tool, None);
    }

    #[test]
    fn last_tool_flag_wins() {
        let parsed = parse_launch_args_impl(
            &args(&["exe", "--tool", "merge", "--tool", "convert", "/a.pdf"]),
            |p| p == "/a.pdf",
        );
        assert_eq!(parsed.tool, Some("convert".to_string()));
        assert_eq!(parsed.files, vec!["/a.pdf"]);
    }

    #[test]
    fn empty_args_parse_to_nothing() {
        let parsed = parse_launch_args_impl(&args(&["exe"]), |_| true);
        assert_eq!(parsed.files, Vec::<String>::new());
        assert_eq!(parsed.tool, None);
    }

    // ── aggregation timing (paused tokio clock, fully deterministic) ───────

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn single_launch_flushes_after_one_debounce_window() {
        let agg = IntentAggregator::new();
        let generation = agg.add("merge", strings(&["a"]), Instant::now());
        let start = Instant::now();
        let flushed = debounce_intent_batch(&agg, "merge", generation).await;
        assert_eq!(flushed, Some(strings(&["a"])));
        assert_eq!(Instant::now().duration_since(start), INTENT_DEBOUNCE_WINDOW);
        // Batch is gone afterwards.
        assert_eq!(
            agg.take_if_quiescent("merge", generation, Instant::now()),
            None
        );
    }

    #[tokio::test(start_paused = true)]
    async fn superseded_sleeper_defers_to_the_newest_one() {
        let agg = IntentAggregator::new();
        let g1 = agg.add("merge", strings(&["a"]), Instant::now());
        tokio::time::advance(Duration::from_millis(300)).await;
        let g2 = agg.add("merge", strings(&["b"]), Instant::now());

        // t=500ms: sleeper 1 wakes — superseded by g2, must not flush.
        tokio::time::advance(Duration::from_millis(200)).await;
        assert_eq!(agg.take_if_quiescent("merge", g1, Instant::now()), None);

        // t=800ms: sleeper 2 wakes — quiescent, flushes the merged batch.
        tokio::time::advance(Duration::from_millis(300)).await;
        assert_eq!(
            agg.take_if_quiescent("merge", g2, Instant::now()),
            Some(strings(&["a", "b"]))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_multi_select_launches_yield_exactly_one_batch() {
        let agg = IntentAggregator::new();
        let f1 = async {
            let g = agg.add("compress", strings(&["a"]), Instant::now());
            debounce_intent_batch(&agg, "compress", g).await
        };
        let f2 = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let g = agg.add("compress", strings(&["b"]), Instant::now());
            debounce_intent_batch(&agg, "compress", g).await
        };
        let f3 = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let g = agg.add("compress", strings(&["c"]), Instant::now());
            debounce_intent_batch(&agg, "compress", g).await
        };
        let (r1, r2, r3) = tokio::join!(f1, f2, f3);
        assert_eq!(r1, None);
        assert_eq!(r2, None);
        assert_eq!(r3, Some(strings(&["a", "b", "c"])));
    }

    #[tokio::test(start_paused = true)]
    async fn distinct_intents_aggregate_independently() {
        let agg = IntentAggregator::new();
        let gm = agg.add("merge", strings(&["a"]), Instant::now());
        let gc = agg.add("convert", strings(&["b"]), Instant::now());
        let (rm, rc) = tokio::join!(
            debounce_intent_batch(&agg, "merge", gm),
            debounce_intent_batch(&agg, "convert", gc)
        );
        assert_eq!(rm, Some(strings(&["a"])));
        assert_eq!(rc, Some(strings(&["b"])));
    }

    #[tokio::test(start_paused = true)]
    async fn hard_cap_flushes_a_sustained_storm() {
        let agg = IntentAggregator::new();
        // Launches every 400ms keep superseding each other; the sliding
        // window alone would never flush. Simulate each sleeper's wake-up
        // check explicitly.
        let mut generations = Vec::new();
        let mut files = Vec::new();
        for i in 0..9 {
            // arrivals at t = 0, 400, ..., 3200ms
            if i > 0 {
                tokio::time::advance(Duration::from_millis(400)).await;
            }
            let name = format!("f{}", i);
            files.push(name.clone());
            generations.push(agg.add("merge", vec![name], Instant::now()));
        }
        // Sleepers 0..=6 woke at t = 500, 900, ..., 2900ms — all superseded
        // and all under the cap, so none of them flushed. Verify the last two
        // wake-ups: sleeper 7 wakes at t=3300ms (past first_at + 3s) and
        // flushes the whole batch despite being superseded by arrival 8.
        tokio::time::advance(Duration::from_millis(100)).await; // t=3300ms
        assert_eq!(
            agg.take_if_quiescent("merge", generations[7], Instant::now()),
            Some(files.clone())
        );
        // Sleeper 8 wakes at t=3700ms: batch already flushed, nothing left.
        tokio::time::advance(Duration::from_millis(400)).await;
        assert_eq!(
            agg.take_if_quiescent("merge", generations[8], Instant::now()),
            None
        );
    }

    #[tokio::test(start_paused = true)]
    async fn under_cap_wakeups_do_not_flush_while_superseded() {
        let agg = IntentAggregator::new();
        let g1 = agg.add("merge", strings(&["a"]), Instant::now());
        tokio::time::advance(Duration::from_millis(400)).await;
        let _g2 = agg.add("merge", strings(&["b"]), Instant::now());
        // t=500ms: sleeper 1 superseded, well under the cap.
        tokio::time::advance(Duration::from_millis(100)).await;
        assert_eq!(agg.take_if_quiescent("merge", g1, Instant::now()), None);
    }

    #[tokio::test(start_paused = true)]
    async fn stale_generation_from_flushed_batch_cannot_steal_a_new_batch() {
        let agg = IntentAggregator::new();
        let g1 = agg.add("merge", strings(&["a"]), Instant::now());
        assert_eq!(
            debounce_intent_batch(&agg, "merge", g1).await,
            Some(strings(&["a"]))
        );
        // A new batch starts; the monotonic generation counter guarantees the
        // old token can never match it.
        let g2 = agg.add("merge", strings(&["b"]), Instant::now());
        assert_ne!(g1, g2);
        assert_eq!(agg.take_if_quiescent("merge", g1, Instant::now()), None);
        assert_eq!(
            agg.take_if_quiescent("merge", g2, Instant::now()),
            Some(strings(&["b"]))
        );
    }
}
