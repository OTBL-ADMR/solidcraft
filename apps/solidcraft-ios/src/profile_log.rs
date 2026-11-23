//! While Preferences › Graphics › Profile operations is on, the profiler's report is written to
//! the Documents folder, in `Profile/` (Files: On My iPad › SolidCraft › Profile), so it can be
//! read and compared without the desktop's control channel:
//!
//! - `report.json`: what `diagnostics.profile` answers on the desktop (self time per category
//!   and per span name, the most first), so the two can be compared line by line;
//! - `trace.json`: the Chrome trace of the same spans, for chrome://tracing or Perfetto.
//!
//! Written at most every [`ProfileLog::INTERVAL`] and only when more spans were recorded. A
//! subfolder, so neither shows in Open (which lists the folder's own files, `.json` designs among
//! them).

use std::path::Path;
use std::time::{Duration, Instant};

use crate::files::Library;

/// The folder in the library the report goes to.
pub const FOLDER: &str = "Profile";
/// Rows of the report: the span names with the most self time.
const TOP: usize = 60;

#[derive(Default)]
pub struct ProfileLog {
    last_write: Option<Instant>,
    /// Spans the last written report covered.
    spans_written: Option<u64>,
}

impl ProfileLog {
    pub const INTERVAL: Duration = Duration::from_secs(2);

    /// Whether the report is due: profiling on, the last write old enough, and `spans` more
    /// than were written.
    pub fn due(&self, on: bool, spans: u64, now: Instant) -> bool {
        on && self.last_write.is_none_or(|at| now.saturating_duration_since(at) >= Self::INTERVAL) && self.spans_written != Some(spans)
    }

    /// Write the report when it is due. Errors (a full disk) are logged, not shown: profiling
    /// is a diagnosis aid.
    pub fn tick(&mut self, on: bool, library: &Library, now: Instant) {
        // Cheap test first: no report is built while off or between intervals.
        if !on || self.last_write.is_some_and(|at| now.saturating_duration_since(at) < Self::INTERVAL) {
            return;
        }
        let report = solidcraft_engine::profile::report();
        let out = solidcraft_engine::schema::diagnostics::ProfileReport::new(&report, TOP);
        if !self.due(on, out.spans, now) {
            self.last_write = Some(now);
            return;
        }
        self.last_write = Some(now);
        match write(&library.path_of(FOLDER), &out) {
            Ok(()) => self.spans_written = Some(out.spans),
            Err(e) => log::warn!("SolidCraft: profile report not written: {e}"),
        }
    }
}

fn write(dir: &Path, out: &solidcraft_engine::schema::diagnostics::ProfileReport) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(out).map_err(std::io::Error::other)?;
    std::fs::write(dir.join("report.json"), json)?;
    let mut trace = Vec::new();
    solidcraft_engine::profile::write_chrome_trace(&mut trace)?;
    std::fs::write(dir.join("trace.json"), trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_due_when_on_with_new_spans_and_not_more_often_than_the_interval() {
        let start = Instant::now();
        let mut log = ProfileLog::default();
        assert!(!log.due(false, 5, start), "off: never");
        assert!(log.due(true, 5, start));
        log.last_write = Some(start);
        log.spans_written = Some(5);
        assert!(!log.due(true, 9, start + Duration::from_millis(500)), "too soon");
        assert!(!log.due(true, 5, start + ProfileLog::INTERVAL), "nothing new");
        assert!(log.due(true, 9, start + ProfileLog::INTERVAL));
    }

    #[test]
    fn the_report_and_trace_are_written_in_their_folder_and_not_listed_by_open() {
        let root = std::env::temp_dir().join(format!("solidcraft-ios-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = Library { root: root.clone() };
        let mut log = ProfileLog::default();
        log.tick(true, &library, Instant::now());
        let dir = library.path_of(FOLDER);
        assert!(dir.join("report.json").is_file());
        assert!(dir.join("trace.json").is_file());
        let report: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("report.json")).unwrap_or_default()).unwrap_or_default();
        assert!(report.get("by_category").is_some(), "{report}");
        // Open lists the folder's own files: the subfolder and what is in it are not there.
        assert!(library.list(&[]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
