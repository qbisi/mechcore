use super::{Failure, Side};
use serde::Serialize;
use serde_json::{json, value::RawValue};
use std::{fs::File, io::Write};

pub(super) struct Log(File);

#[derive(Serialize)]
struct Record<'a, T: ?Sized> {
    side: Side,
    direction: &'static str,
    phase: &'static str,
    message: &'a T,
}

impl Log {
    pub(super) fn open(path: &str) -> Result<Self, Failure> {
        File::create_new(path)
            .map(Self)
            .map_err(|e| Failure::failed(format!("cannot create arena log {path:?}: {e}")))
    }

    pub(super) fn record<T: Serialize + ?Sized>(
        &self,
        side: Side,
        direction: &'static str,
        phase: &'static str,
        message: &T,
    ) -> Result<(), Failure> {
        let record = Record {
            side,
            direction,
            phase,
            message,
        };
        let mut row = serde_json::to_vec(&record).map_err(|e| Failure::failed(e.to_string()))?;
        row.push(b'\n');
        (&self.0)
            .write_all(&row)
            .map_err(|e| Failure::failed(format!("cannot write arena log: {e}")))
    }

    pub(super) fn received(
        &self,
        side: Side,
        phase: &'static str,
        bytes: &[u8],
    ) -> Result<(), Failure> {
        let direction = "bot_to_arena";
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                if phase == "play"
                    && let Ok(message) = serde_json::from_str::<&RawValue>(text)
                {
                    return self.record(side, direction, phase, message);
                }
                self.record(side, direction, phase, text)
            }
            Err(_) => self.record(side, direction, phase, &json!({"bytes":bytes})),
        }
    }
}
