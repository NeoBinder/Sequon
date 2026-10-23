//! Free-form, adapter-namespaced extension payloads.
//!
//! extras is the only forward-compatible extension point at the envelope level.
//! Core code never interprets it and it never contributes to a digest. The
//! representation is intentional:
//!
//! - objects are BTreeMap values, so keys are sorted lexicographically;
//! - integers stay integers, and a floating-point number fails to deserialize
//!   with an error that names the canonical JSON profile rule;
//! - arrays keep their element order;
//! - unknown namespaces are preserved as data.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Upper bound for capacity reserved from a container size hint.
///
/// A hostile document can claim a huge array or object length. Reserving that
/// amount up front would be an unbounded allocation, so the hint is only
/// advisory and the contents grow as they are actually read.
const CAPACITY_HINT_LIMIT: usize = 1024;

/// A JSON value stored inside extras.
///
/// The canonical JSON profile forbids floating-point numbers everywhere, so
/// this type has no floating-point variant and deserializing one fails.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum JsonValue {
    /// JSON null.
    #[default]
    Null,
    /// A JSON boolean.
    Bool(bool),
    /// A JSON integer that fits in the i64 range.
    Integer(i64),
    /// A JSON integer greater than i64::MAX.
    Unsigned(u64),
    /// A JSON string.
    String(String),
    /// A JSON array; element order is preserved.
    Array(Vec<JsonValue>),
    /// A JSON object; keys are sorted because the representation is a BTreeMap.
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    /// Returns the value as a boolean when it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as an i64 when it is an integer in that range.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as a u64 when it is a non-negative integer.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Integer(value) => u64::try_from(*value).ok(),
            JsonValue::Unsigned(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as a string slice when it is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the value as an array when it is one.
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the value as an object when it is one.
    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(values) => Some(values),
            _ => None,
        }
    }

    /// Returns whether the value is JSON null.
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }
}

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        JsonValue::Bool(value)
    }
}

impl From<i64> for JsonValue {
    fn from(value: i64) -> Self {
        JsonValue::Integer(value)
    }
}

impl From<i32> for JsonValue {
    fn from(value: i32) -> Self {
        JsonValue::Integer(i64::from(value))
    }
}

impl From<u32> for JsonValue {
    fn from(value: u32) -> Self {
        JsonValue::Integer(i64::from(value))
    }
}

impl From<u64> for JsonValue {
    fn from(value: u64) -> Self {
        match i64::try_from(value) {
            Ok(value) => JsonValue::Integer(value),
            Err(_) => JsonValue::Unsigned(value),
        }
    }
}

impl From<String> for JsonValue {
    fn from(value: String) -> Self {
        JsonValue::String(value)
    }
}

impl From<&str> for JsonValue {
    fn from(value: &str) -> Self {
        JsonValue::String(value.to_owned())
    }
}

impl From<Vec<JsonValue>> for JsonValue {
    fn from(values: Vec<JsonValue>) -> Self {
        JsonValue::Array(values)
    }
}

impl From<BTreeMap<String, JsonValue>> for JsonValue {
    fn from(values: BTreeMap<String, JsonValue>) -> Self {
        JsonValue::Object(values)
    }
}

/// Error text used when a floating-point number appears in canonical JSON.
fn floats_not_allowed(value: f64) -> String {
    format!(
        "floating-point number {value} is not allowed in canonical Sequon JSON; integers must stay integers and the canonical JSON profile has no floating-point representation"
    )
}

/// Error text used when an integer does not fit the supported range.
fn integer_out_of_range() -> &'static str {
    "integer is outside the supported range (i64 or u64)"
}

impl Serialize for JsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            JsonValue::Null => serializer.serialize_unit(),
            JsonValue::Bool(value) => serializer.serialize_bool(*value),
            JsonValue::Integer(value) => serializer.serialize_i64(*value),
            JsonValue::Unsigned(value) => serializer.serialize_u64(*value),
            JsonValue::String(value) => serializer.serialize_str(value),
            JsonValue::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(value)?;
                }
                sequence.end()
            }
            JsonValue::Object(values) => {
                let mut map = serializer.serialize_map(Some(values.len()))?;
                for (key, value) in values {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

struct JsonValueVisitor;

impl<'de> Visitor<'de> for JsonValueVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without floating-point numbers")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::Integer(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(match i64::try_from(value) {
            Ok(value) => JsonValue::Integer(value),
            Err(_) => JsonValue::Unsigned(value),
        })
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match i64::try_from(value) {
            Ok(value) => Ok(JsonValue::Integer(value)),
            Err(_) => Err(E::custom(integer_out_of_range())),
        }
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match u64::try_from(value) {
            Ok(value) => self.visit_u64(value),
            Err(_) => Err(E::custom(integer_out_of_range())),
        }
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Err(E::custom(floats_not_allowed(f64::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Err(E::custom(floats_not_allowed(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        JsonValue::deserialize(deserializer)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(JsonValue::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let hint = sequence.size_hint().unwrap_or(0).min(CAPACITY_HINT_LIMIT);
        let mut values = Vec::with_capacity(hint);
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(JsonValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, JsonValue>()? {
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!(
                    "duplicate object key '{key}' is not allowed in canonical Sequon JSON"
                )));
            }
        }
        Ok(JsonValue::Object(values))
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonValueVisitor)
    }
}

/// Adapter-namespaced extension payloads attached to a record.
///
/// Serialized as a JSON object whose keys are sorted lexicographically because
/// the payload is a BTreeMap. Each adapter writes only its own namespace; the
/// core validates and passes the value through unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Extras(BTreeMap<String, JsonValue>);

impl Extras {
    /// Creates an empty extras map.
    pub fn new() -> Self {
        Extras(BTreeMap::new())
    }

    /// Returns whether no namespace is present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of namespaces.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the payload of one namespace.
    pub fn get(&self, namespace: &str) -> Option<&JsonValue> {
        self.0.get(namespace)
    }

    /// Returns a mutable payload of one namespace.
    pub fn get_mut(&mut self, namespace: &str) -> Option<&mut JsonValue> {
        self.0.get_mut(namespace)
    }

    /// Inserts a namespace payload, returning the previous value if any.
    pub fn insert(&mut self, namespace: impl Into<String>, value: JsonValue) -> Option<JsonValue> {
        self.0.insert(namespace.into(), value)
    }

    /// Removes and returns one namespace payload.
    pub fn remove(&mut self, namespace: &str) -> Option<JsonValue> {
        self.0.remove(namespace)
    }

    /// Iterates namespaces in sorted key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &JsonValue)> {
        self.0.iter()
    }

    /// Returns the underlying map.
    pub fn as_map(&self) -> &BTreeMap<String, JsonValue> {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Extras {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ExtrasVisitor;

        impl<'de> Visitor<'de> for ExtrasVisitor {
            type Value = Extras;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object of adapter namespaces")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, JsonValue>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(de::Error::custom(format!(
                            "duplicate object key '{key}' is not allowed in canonical Sequon JSON"
                        )));
                    }
                }
                Ok(Extras(values))
            }
        }

        deserializer.deserialize_map(ExtrasVisitor)
    }
}

impl From<BTreeMap<String, JsonValue>> for Extras {
    fn from(values: BTreeMap<String, JsonValue>) -> Self {
        Extras(values)
    }
}

impl FromIterator<(String, JsonValue)> for Extras {
    fn from_iter<T: IntoIterator<Item = (String, JsonValue)>>(iter: T) -> Self {
        Extras(BTreeMap::from_iter(iter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_keys_are_sorted_regardless_of_insertion_order() {
        let mut first = BTreeMap::new();
        first.insert("zeta".to_owned(), JsonValue::from(1_i64));
        first.insert("alpha".to_owned(), JsonValue::from("kept"));
        let value = JsonValue::Object(first);
        let object = value.as_object().expect("object");
        assert_eq!(
            object.keys().cloned().collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn integer_helpers_cover_signed_and_unsigned_ranges() {
        assert_eq!(JsonValue::from(-7_i64).as_i64(), Some(-7));
        assert_eq!(JsonValue::from(u64::MAX).as_u64(), Some(u64::MAX));
        assert_eq!(JsonValue::from(u64::MAX).as_i64(), None);
        assert_eq!(JsonValue::from(5_u64), JsonValue::Integer(5));
    }

    #[test]
    fn extras_iterates_in_sorted_order() {
        let mut extras = Extras::new();
        extras.insert("snapgene", JsonValue::from(2_u32));
        extras.insert("genbank", JsonValue::from(1_u32));
        assert_eq!(
            extras
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["genbank", "snapgene"]
        );
        assert_eq!(extras.len(), 2);
        assert!(!extras.is_empty());
    }

    #[test]
    fn default_value_is_null() {
        assert!(JsonValue::default().is_null());
    }
}
