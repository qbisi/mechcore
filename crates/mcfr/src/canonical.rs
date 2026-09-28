use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, Result};

pub(crate) const HASH_BYTES: usize = 32;

/// The canonical bytes of `value`: its JSON with every object's keys in byte
/// order. `serde_json` keeps a `Value`'s object as a `BTreeMap` unless its
/// `preserve_order` feature is on, so the keys come out sorted as they are
/// serialized; the test below holds that.
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&serde_json::to_value(value)?)?)
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

/// One tick's hash: its number, then the canonical bytes of its state and of
/// its events. The domain string names the definition, which has not changed
/// since format 0.7.0, so a hash computed then is the hash computed now.
pub(crate) fn tick_hash(tick: u32, state: &[u8], events: &[u8]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new("content-tick-0.7.0");
    hasher.update(&tick.to_le_bytes());
    hasher.update(state);
    hasher.update(events);
    hasher.finalize()
}

/// The whole fight's hash: the tick count, then every tick's hash in order.
pub(crate) fn result_hash(tick_hashes: &[[u8; HASH_BYTES]]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new("content-result-0.7.0");
    let tick_count = u32::try_from(tick_hashes.len()).expect("tick hash count exceeds u32");
    hasher.update(&tick_count.to_le_bytes());
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
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
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

#[cfg(test)]
mod tests {
    /// A struct's fields come out in byte order, whatever order it declares
    /// them in, which is what makes [`super::encode`] canonical without
    /// sorting anything itself.
    #[test]
    fn a_struct_encodes_its_keys_in_byte_order() {
        #[derive(serde::Serialize)]
        struct Declared {
            zeta: u8,
            alpha: u8,
            #[serde(rename = "Beta")]
            beta: u8,
        }
        let declared = Declared {
            zeta: 1,
            alpha: 2,
            beta: 3,
        };
        assert_eq!(
            super::encode(&declared).unwrap(),
            br#"{"Beta":3,"alpha":2,"zeta":1}"#
        );
    }
}
