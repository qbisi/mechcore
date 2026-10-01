//! One number per tick, written the way it changes.
//!
//! Most of what a recording holds stands still from one tick to the next: a
//! unit idles, a tower keeps its life, a shield keeps its radius. A track
//! therefore writes its first value, then each tick's difference from the one
//! before, and folds a run of unchanged ticks into one string holding the
//! run's length. `[120, 3, "4", -2]` is the six ticks `120, 123, 123, 123,
//! 123, 121`: the page reads it back with a running sum.

use serde::{Serialize, Serializer, ser::SerializeSeq};

/// A run shorter than this is written as zeros, which is no longer.
const SHORTEST_HELD_RUN: u32 = 3;

/// The values of one field of one object, one per tick of its life.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Track {
    values: Vec<i64>,
}

impl Track {
    pub fn push(&mut self, value: i64) {
        self.values.push(value);
    }

    /// The last value, which a tick the object skipped repeats.
    #[must_use]
    pub fn last(&self) -> Option<i64> {
        self.values.last().copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub fn values(&self) -> &[i64] {
        &self.values
    }

    /// Whether every value is the same, which a reader may take as a constant.
    #[must_use]
    pub fn is_constant(&self) -> bool {
        self.values.windows(2).all(|pair| pair[0] == pair[1])
    }

    /// The written form: a first value, then differences and held runs.
    #[must_use]
    pub fn steps(&self) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut previous = 0;
        let mut held = 0_u32;
        for (index, &value) in self.values.iter().enumerate() {
            if index > 0 && value == previous {
                held += 1;
                continue;
            }
            flush(&mut steps, &mut held);
            steps.push(Step::Change(value - previous));
            previous = value;
        }
        flush(&mut steps, &mut held);
        steps
    }

    /// Reads the written form back, as the page does.
    #[must_use]
    pub fn from_steps(steps: &[Step]) -> Self {
        let mut values = Vec::new();
        let mut current = 0;
        for step in steps {
            match *step {
                Step::Change(difference) => {
                    current += difference;
                    values.push(current);
                }
                Step::Hold(run) => values.extend((0..run).map(|_| current)),
            }
        }
        Self { values }
    }
}

fn flush(steps: &mut Vec<Step>, held: &mut u32) {
    if *held >= SHORTEST_HELD_RUN {
        steps.push(Step::Hold(*held));
    } else {
        steps.extend((0..*held).map(|_| Step::Change(0)));
    }
    *held = 0;
}

/// One entry of a written track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The difference from the tick before; for the first tick, the value.
    Change(i64),
    /// This many more ticks at the value already reached.
    Hold(u32),
}

impl Serialize for Track {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let steps = self.steps();
        let mut sequence = serializer.serialize_seq(Some(steps.len()))?;
        for step in steps {
            match step {
                Step::Change(difference) => sequence.serialize_element(&difference)?,
                Step::Hold(run) => sequence.serialize_element(&run.to_string())?,
            }
        }
        sequence.end()
    }
}

#[cfg(test)]
mod tests {
    use super::{Step, Track};

    fn track(values: &[i64]) -> Track {
        let mut track = Track::default();
        for &value in values {
            track.push(value);
        }
        track
    }

    #[test]
    fn a_track_writes_differences_and_folds_a_held_run() {
        let written = track(&[120, 123, 123, 123, 123, 123, 121]);
        assert_eq!(
            written.steps(),
            [
                Step::Change(120),
                Step::Change(3),
                Step::Hold(4),
                Step::Change(-2)
            ]
        );
        assert_eq!(
            serde_json::to_string(&written).unwrap(),
            r#"[120,3,"4",-2]"#
        );
    }

    #[test]
    fn a_short_run_stays_zeros() {
        assert_eq!(
            track(&[5, 5, 5, 6]).steps(),
            [
                Step::Change(5),
                Step::Change(0),
                Step::Change(0),
                Step::Change(1)
            ]
        );
    }

    #[test]
    fn a_track_reads_back_as_written() {
        for values in [
            vec![],
            vec![7],
            vec![0, 0, 0, 0, 0],
            vec![-3, -3, 9, 9, 9, 9, 9, 9, -40, 2, 2],
        ] {
            let written = track(&values);
            assert_eq!(Track::from_steps(&written.steps()), written);
        }
    }
}
