//! Map-only decoding around ordinary derive-backed serialized data.

use std::ops::{Deref, DerefMut};

use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize};

struct ObjectDeserializer<D>(D);

impl<'de, D: Deserializer<'de>> Deserializer<'de> for ObjectDeserializer<D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.0.deserialize_map(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// Serialized object data that cannot be decoded from positional sequences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Object<T>(T);

impl<T> Object<T> {
    /// Consumes the map-only wrapper without cloning the decoded value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Object<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Object<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(ObjectDeserializer(deserializer)).map(Self)
    }
}
