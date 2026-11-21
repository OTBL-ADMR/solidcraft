//! Settings are written soon after they change, not only when the app closes: iPadOS ends a
//! suspended app without telling it, so eframe's save at exit may never run.

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct PrefsWriter {
    written: Option<String>,
    last_write: Option<Instant>,
    input_before: bool,
    pending_until: Option<Instant>,
}

impl PrefsWriter {
    /// At most one write in this long.
    pub const INTERVAL: Duration = Duration::from_secs(2);

    /// Whether this frame's settings are worth building: input came this frame or the one before,
    /// or a waiting change's time has come.
    pub fn worth_checking(&mut self, input: bool, now: Instant) -> bool {
        let worth = input || self.input_before || self.pending_until.is_some_and(|at| now >= at);
        self.input_before = input;
        worth
    }

    /// What to do with `prefs` now: `Write` when they changed and the last write is old enough,
    /// `Wait(d)` when they changed sooner (ask for a frame in `d`), `Nothing` when unchanged.
    pub fn due(&mut self, prefs: &str, now: Instant) -> Due {
        if self.written.as_deref() == Some(prefs) {
            self.pending_until = None;
            return Due::Nothing;
        }
        match self.last_write.map(|at| Self::INTERVAL.saturating_sub(now.saturating_duration_since(at))) {
            Some(wait) if !wait.is_zero() => {
                self.pending_until = Some(now + wait);
                Due::Wait(wait)
            }
            _ => Due::Write,
        }
    }

    pub fn wrote(&mut self, prefs: String, now: Instant) {
        self.written = Some(prefs);
        self.last_write = Some(now);
        self.pending_until = None;
    }

    /// The settings read at start count as written: an unchanged start writes nothing.
    pub fn read(&mut self, prefs: String) {
        self.written = Some(prefs);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Due {
    Nothing,
    Write,
    Wait(Duration),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_written_at_once_then_at_most_every_interval() {
        let start = Instant::now();
        let mut w = PrefsWriter::default();
        assert_eq!(w.due("a", start), Due::Write);
        w.wrote("a".into(), start);
        assert_eq!(w.due("a", start), Due::Nothing);
        let soon = start + Duration::from_millis(500);
        assert_eq!(w.due("b", soon), Due::Wait(PrefsWriter::INTERVAL - Duration::from_millis(500)));
        assert_eq!(w.due("b", start + PrefsWriter::INTERVAL), Due::Write);
    }

    #[test]
    fn settings_are_built_after_input_and_when_a_waiting_change_is_due() {
        let start = Instant::now();
        let mut w = PrefsWriter::default();
        assert!(!w.worth_checking(false, start));
        assert!(w.worth_checking(true, start));
        // The frame after input still looks: a click's change can land a frame late.
        assert!(w.worth_checking(false, start));
        assert!(!w.worth_checking(false, start));
        w.wrote("a".into(), start);
        let soon = start + Duration::from_millis(100);
        assert!(matches!(w.due("b", soon), Due::Wait(_)));
        assert!(!w.worth_checking(false, soon));
        assert!(w.worth_checking(false, start + PrefsWriter::INTERVAL));
    }

    #[test]
    fn settings_read_at_start_are_not_written_again() {
        let mut w = PrefsWriter::default();
        w.read("stored".into());
        assert_eq!(w.due("stored", Instant::now()), Due::Nothing);
        assert_eq!(w.due("changed", Instant::now()), Due::Write);
    }
}
