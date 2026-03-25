//! Criterion `Measurement` implementation backed by platform counters.

use std::time::Duration;

use criterion::measurement::{Measurement, ValueFormatter, WallTime};

use crate::counters::{CycleCounter, DefaultCounter};

/// Criterion measurement backed by a platform cycle counter.
///
/// Wraps a [`CycleCounter`] and delegates formatting to [`WallTime`] so that
/// criterion's built-in duration display is preserved.
pub struct MantisMeasurement<C: CycleCounter> {
    pub(crate) counter: C,
    wall: WallTime,
}

impl<C: CycleCounter> MantisMeasurement<C> {
    /// Create a new measurement with the given counter.
    pub fn new(counter: C) -> Self {
        Self {
            counter,
            wall: WallTime,
        }
    }
}

impl<C: CycleCounter> Measurement for MantisMeasurement<C> {
    type Intermediate = (u64, std::time::Instant);
    type Value = Duration;

    fn start(&self) -> Self::Intermediate {
        let cycles = self.counter.start();
        let wall_start = std::time::Instant::now();
        (cycles, wall_start)
    }

    fn end(&self, i: Self::Intermediate) -> Self::Value {
        let wall_elapsed = i.1.elapsed();
        let _ = self.counter.elapsed(i.0);
        wall_elapsed
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        *v1 + *v2
    }

    fn zero(&self) -> Self::Value {
        Duration::ZERO
    }

    #[expect(clippy::cast_precision_loss, reason = "nanosecond counts fit in f64 for typical benchmark durations")]
    fn to_f64(&self, value: &Self::Value) -> f64 {
        value.as_nanos() as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        self.wall.formatter()
    }
}

/// Default measurement using the platform-selected counter.
pub type DefaultMeasurement = MantisMeasurement<DefaultCounter>;

impl DefaultMeasurement {
    /// Create a default measurement for the current platform.
    #[must_use]
    pub fn platform_default() -> Self {
        Self::new(DefaultCounter::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::InstantCounter;

    #[test]
    fn measurement_creation() {
        let m = MantisMeasurement::new(InstantCounter::new());
        let _start = m.counter.start();
    }

    #[test]
    fn measurement_start_end_returns_duration() {
        let m = MantisMeasurement::new(InstantCounter::new());
        let i = m.start();
        let d = m.end(i);
        assert!(d.as_nanos() < 1_000_000_000);
    }

    #[test]
    fn measurement_add_and_zero() {
        let m = MantisMeasurement::new(InstantCounter::new());
        let z = m.zero();
        let d = Duration::from_nanos(100);
        assert_eq!(m.add(&z, &d), d);
    }
}
