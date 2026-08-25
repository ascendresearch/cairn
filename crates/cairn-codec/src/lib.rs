//! Canonical JSON V1 encoding and strict decoding.
//!
//! V1 orders object keys by UTF-8 byte sequence, emits no insignificant whitespace, rejects
//! duplicate keys, and permits only integer JSON numbers. Floating-point evidence must use an
//! explicit schema representation such as exact bits, scaled integers, or specified strings.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::{Map, Number, Value};
use thiserror::Error;

/// Stable identifier for the first canonical encoding.
pub const ENCODING_ID: &str = "cairn.canonical-json.v1";

/// Canonical JSON encoding or decoding failure.
#[derive(Debug, Error)]
pub enum CodecError {
    /// The value cannot be represented under the V1 rules.
    #[error("JSON serialization failed: {0}")]
    Serialize(#[source] serde_json::Error),
    /// The input is not valid under the strict JSON rules.
    #[error("JSON decoding failed: {0}")]
    Decode(#[source] serde_json::Error),
    /// The bytes parse but are not the one canonical representation of the value.
    #[error("input is valid JSON but not canonical {ENCODING_ID}")]
    NonCanonical,
    /// Raw JSON floating-point values are intentionally excluded from V1.
    #[error(
        "raw floating-point JSON numbers are not supported; use an exact schema representation"
    )]
    RawFloat,
}

/// Encodes a serializable value as canonical JSON V1.
///
/// # Errors
///
/// Returns [`CodecError::Serialize`] when Serde cannot represent the value, or
/// [`CodecError::RawFloat`] when it contains a raw floating-point JSON number.
pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let value = serde_json::to_value(value).map_err(CodecError::Serialize)?;
    encode_value(&value)
}

/// Strictly decodes canonical JSON V1 into a typed value.
///
/// Valid but non-canonical JSON is rejected rather than silently normalized.
///
/// # Errors
///
/// Returns a [`CodecError`] when the bytes are invalid, ambiguous, non-canonical, contain raw
/// floating-point numbers, or cannot be converted into `T`.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    let value = parse_unique_value(bytes)?;
    if encode_value(&value)? != bytes {
        return Err(CodecError::NonCanonical);
    }
    serde_json::from_value(value).map_err(CodecError::Decode)
}

/// Parses strict JSON and returns its canonical V1 bytes.
///
/// This is an explicit normalization operation; durable readers should normally use
/// [`from_slice`] so non-canonical persisted bytes remain visible as a defect.
///
/// # Errors
///
/// Returns a [`CodecError`] when the bytes are invalid, contain duplicate keys, or contain raw
/// floating-point numbers.
pub fn canonicalize(bytes: &[u8]) -> Result<Vec<u8>, CodecError> {
    encode_value(&parse_unique_value(bytes)?)
}

fn parse_unique_value(bytes: &[u8]) -> Result<Value, CodecError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = UniqueValue::deserialize(&mut deserializer)
        .map_err(|error| {
            if error.to_string().contains("raw floating-point") {
                CodecError::RawFloat
            } else {
                CodecError::Decode(error)
            }
        })?
        .0;
    deserializer.end().map_err(CodecError::Decode)?;
    Ok(value)
}

fn encode_value(value: &Value) -> Result<Vec<u8>, CodecError> {
    let mut output = Vec::new();
    write_value(value, &mut output)?;
    Ok(output)
}

fn write_value(value: &Value, output: &mut Vec<u8>) -> Result<(), CodecError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => write_integer(number, output)?,
        Value::String(string) => {
            serde_json::to_writer(output, string).map_err(CodecError::Serialize)?;
        }
        Value::Array(items) => {
            output.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_value(item, output)?;
            }
            output.push(b']');
        }
        Value::Object(object) => {
            output.push(b'{');
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key).map_err(CodecError::Serialize)?;
                output.push(b':');
                write_value(item, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

fn write_integer(number: &Number, output: &mut Vec<u8>) -> Result<(), CodecError> {
    if !(number.is_i64() || number.is_u64()) {
        return Err(CodecError::RawFloat);
    }
    output.extend_from_slice(number.to_string().as_bytes());
    Ok(())
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

struct UniqueValueVisitor;

impl<'de> serde::de::Visitor<'de> for UniqueValueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate keys or raw floating-point numbers")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Err(E::custom(
            "raw floating-point JSON numbers are not supported",
        ))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        UniqueValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value::<UniqueValue>()?;
            if values.insert(key.clone(), value.0).is_some() {
                return Err(serde::de::Error::custom(format_args!(
                    "duplicate object key {key:?}"
                )));
            }
        }
        Ok(UniqueValue(Value::Object(
            values.into_iter().collect::<Map<String, Value>>(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::{CodecError, canonicalize, from_slice, to_vec};

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Example {
        name: String,
        count: u64,
    }

    #[test]
    fn canonical_encoding_sorts_keys_and_has_no_padding() {
        let input = br#"{ "z": [3, 2], "a": {"b": true, "a": null} }"#;
        assert_eq!(
            canonicalize(input).expect("canonicalize"),
            br#"{"a":{"a":null,"b":true},"z":[3,2]}"#
        );
    }

    #[test]
    fn strict_decode_rejects_noncanonical_bytes() {
        let error = from_slice::<Example>(br#"{"name":"x", "count":1}"#)
            .expect_err("whitespace is noncanonical");
        assert!(matches!(error, CodecError::NonCanonical));
    }

    #[test]
    fn duplicate_keys_are_rejected_before_typed_decode() {
        let error = canonicalize(br#"{"x":1,"x":2}"#).expect_err("duplicate key must fail");
        assert!(error.to_string().contains("duplicate object key"));
    }

    #[test]
    fn raw_floats_are_rejected_on_encode_and_decode() {
        assert!(matches!(to_vec(&1.25_f64), Err(CodecError::RawFloat)));
        assert!(matches!(canonicalize(b"1.25"), Err(CodecError::RawFloat)));
    }

    #[test]
    fn typed_values_round_trip() {
        let value = Example {
            name: "reduction/α".to_owned(),
            count: 7,
        };
        let bytes = to_vec(&value).expect("encode");
        assert_eq!(from_slice::<Example>(&bytes).expect("decode"), value);
    }
}
