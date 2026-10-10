use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, Result};

pub(crate) const HASH_BYTES: usize = 32;

/// The canonical bytes of `value`: its compact JSON, every object's keys in
/// the order its type declares them. Every hashed type declares its fields in
/// byte order, so an object's keys come out sorted, but for an event payload,
/// whose `kind` tag comes first; the tests below hold both.
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    encode_into(&mut out, value)?;
    Ok(out)
}

/// [`encode`] into `out`, cleared first, so a caller encoding every tick
/// keeps one buffer.
pub(crate) fn encode_into<T: Serialize>(out: &mut Vec<u8>, value: &T) -> Result<()> {
    out.clear();
    serde_json::to_writer(out, value)?;
    Ok(())
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
/// its events. The domain string names the definition by its profile.
pub(crate) fn tick_hash(tick: u32, state: &[u8], events: &[u8]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new(concat!(
        "content-tick-0.",
        crate::model::hash_profile!(),
        ".0"
    ));
    hasher.update(&tick.to_le_bytes());
    hasher.update(state);
    hasher.update(events);
    hasher.finalize()
}

/// The whole fight's hash: the tick count, then every tick's hash in order.
pub(crate) fn result_hash(tick_hashes: &[[u8; HASH_BYTES]]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new(concat!(
        "content-result-0.",
        crate::model::hash_profile!(),
        ".0"
    ));
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
    use crate::{EventPayload, ObjectKind, ObjectRef};

    /// The fields of every struct `source` declares, and of every variant of
    /// its `EventPayload`, by the type that declares them.
    fn declared(source: &str) -> Vec<(String, Vec<String>)> {
        let mut types = Vec::new();
        let mut current: Option<(String, String, Vec<String>)> = None;
        let mut in_payload = false;
        for line in source.lines() {
            if line.starts_with("pub enum EventPayload") {
                in_payload = true;
            } else if in_payload && line == "}" {
                in_payload = false;
            }
            if let Some((indent, name, fields)) = &mut current {
                if line == format!("{indent}}}") || line == format!("{indent}}},") {
                    types.push((std::mem::take(name), std::mem::take(fields)));
                    current = None;
                    continue;
                }
                let field = line.trim_start();
                if field.starts_with("//") || field.starts_with("#[") || field.is_empty() {
                    continue;
                }
                let field = field.strip_prefix("pub ").unwrap_or(field);
                if let Some((name, _)) = field.split_once(": ") {
                    fields.push(name.to_owned());
                }
                continue;
            }
            let indent = &line[..line.len() - line.trim_start().len()];
            let Some(head) = line.trim_start().strip_suffix(" {") else {
                continue;
            };
            let name = if let Some(name) = head.strip_prefix("pub struct ") {
                name
            } else if in_payload && indent == "    " && !head.contains(' ') {
                head
            } else {
                continue;
            };
            current = Some((indent.to_owned(), name.to_owned(), Vec::new()));
        }
        types
    }

    /// Every hashed type declares its fields in byte order, which is what
    /// makes [`super::encode`] write every object's keys sorted without
    /// sorting anything itself.
    #[test]
    fn the_model_declares_every_field_in_byte_order() {
        let types = declared(include_str!("model.rs"));
        assert!(types.iter().any(|(name, _)| name == "LiveUnitState"));
        assert!(types.iter().any(|(name, _)| name == "UnitCreated"));
        for (name, fields) in types {
            let mut sorted = fields.clone();
            sorted.sort();
            assert_eq!(fields, sorted, "{name} declares its fields out of order");
        }
    }

    /// An event payload's `kind` is its tag, which comes before its fields.
    #[test]
    fn an_event_payload_writes_its_kind_first() {
        let payload = EventPayload::ProjectileRemoved {
            absorbed_by: Some(ObjectRef::new(ObjectKind::Shield, 3)),
            intercepted: false,
            position: crate::QVec3 { x: 1, y: 2, z: 3 },
        };
        assert_eq!(
            String::from_utf8(super::encode(&payload).unwrap()).unwrap(),
            r#"{"kind":"projectile_removed","absorbed_by":{"id":3,"kind":"shield"},"intercepted":false,"position":{"x":1,"y":2,"z":3}}"#
        );
    }
}
