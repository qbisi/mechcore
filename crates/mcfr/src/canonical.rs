use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{Error, Result};

pub(crate) const HASH_BYTES: usize = 32;

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut value = serde_json::to_value(value)?;
    normalize(&mut value);
    Ok(serde_json::to_vec(&value)?)
}

pub(crate) fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8], label: &str) -> Result<T> {
    let value: T = serde_json::from_slice(bytes)?;
    if encode(&value)? != bytes {
        return Err(Error::invalid(format!(
            "{label} is not encoded in canonical form"
        )));
    }
    Ok(value)
}

pub(crate) fn normalize(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize(value);
            }
        }
        Value::Object(values) => {
            let old = std::mem::take(values);
            let mut entries = old.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, mut value) in entries {
                normalize(&mut value);
                values.insert(key, value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub(crate) struct CanonicalHasher(blake3::Hasher);

impl CanonicalHasher {
    pub(crate) fn new(domain: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"mechcore.mcfr.canonical\0");
        feed(&mut hasher, domain.as_bytes());
        Self(hasher)
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        feed(&mut self.0, bytes);
    }

    pub(crate) fn finalize(self) -> [u8; HASH_BYTES] {
        *self.0.finalize().as_bytes()
    }
}

fn feed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

pub(crate) fn tick_hash(tick: u64, state: &[u8], events: &[u8]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new("tick-v3");
    hasher.update(&tick.to_le_bytes());
    hasher.update(state);
    hasher.update(events);
    hasher.finalize()
}

pub(crate) fn result_hash(
    scenario: &[u8; HASH_BYTES],
    tick_hashes: &[[u8; HASH_BYTES]],
) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new("result-v3");
    hasher.update(scenario);
    hasher.update(&(tick_hashes.len() as u64).to_le_bytes());
    for tick_hash in tick_hashes {
        hasher.update(tick_hash);
    }
    hasher.finalize()
}

pub(crate) fn hex(bytes: &[u8; HASH_BYTES]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(HASH_BYTES * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn parse_hex(value: &str, label: &str) -> Result<[u8; HASH_BYTES]> {
    if value.len() != HASH_BYTES * 2 {
        return Err(Error::invalid(format!("{label} is not a 64-digit hash")));
    }
    let mut output = [0; HASH_BYTES];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (nibble(pair[0], label)? << 4) | nibble(pair[1], label)?;
    }
    Ok(output)
}

fn nibble(value: u8, label: &str) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(Error::invalid(format!(
            "{label} contains a non-canonical hexadecimal digit"
        ))),
    }
}
