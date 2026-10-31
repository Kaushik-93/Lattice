use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Named phase durations for one request, rendered as a `Server-Timing` header.
///
/// Browsers show the header in DevTools and expose it through
/// `PerformanceResourceTiming.serverTiming`, which the web app's perf overlay reads, so
/// the breakdown needs no extra payload field and no extra round trip.
#[derive(Clone, Default)]
pub struct ServerTiming {
    phases: Arc<Mutex<Vec<(&'static str, Duration)>>>,
}

impl ServerTiming {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a phase. Recording the same name twice accumulates, so a handler that hits
    /// the database three times reports one `db` entry with the total.
    pub fn record(&self, name: &'static str, elapsed: Duration) {
        // A poisoned lock only means another thread panicked mid-push; the data is still usable.
        let mut phases = self.phases.lock().unwrap_or_else(|e| e.into_inner());
        match phases.iter_mut().find(|(n, _)| *n == name) {
            Some((_, total)) => *total += elapsed,
            None => phases.push((name, elapsed)),
        }
    }

    /// Times `fut` and records it under `name`.
    pub async fn measure<F: Future>(&self, name: &'static str, fut: F) -> F::Output {
        let start = Instant::now();
        let out = fut.await;
        self.record(name, start.elapsed());
        out
    }

    /// Renders e.g. `auth;dur=0.42, db;dur=1.07, total;dur=1.81` (milliseconds).
    pub fn header_value(&self, total: Duration) -> String {
        let phases = self.phases.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = String::with_capacity(16 * (phases.len() + 1));
        for (name, elapsed) in phases.iter() {
            push_metric(&mut out, name, *elapsed);
            out.push_str(", ");
        }
        push_metric(&mut out, "total", total);
        out
    }
}

fn push_metric(out: &mut String, name: &str, elapsed: Duration) {
    // Writing to a String cannot fail.
    let _ = write!(out, "{name};dur={:.2}", elapsed.as_secs_f64() * 1000.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_phases_in_insertion_order_with_total_last() {
        let t = ServerTiming::new();
        t.record("auth", Duration::from_micros(420));
        t.record("db", Duration::from_micros(1070));
        assert_eq!(
            t.header_value(Duration::from_micros(1810)),
            "auth;dur=0.42, db;dur=1.07, total;dur=1.81"
        );
    }

    #[test]
    fn repeated_phase_names_accumulate() {
        let t = ServerTiming::new();
        t.record("db", Duration::from_millis(1));
        t.record("db", Duration::from_millis(2));
        assert_eq!(
            t.header_value(Duration::from_millis(4)),
            "db;dur=3.00, total;dur=4.00"
        );
    }

    #[test]
    fn clones_share_the_same_phases() {
        let t = ServerTiming::new();
        t.clone().record("db", Duration::from_millis(1));
        assert!(t.header_value(Duration::ZERO).starts_with("db;dur=1.00"));
    }
}
