use serde::{
    Deserialize, Serialize,
    de::{self, SeqAccess, Visitor},
};
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WithAttributes<T, A> {
    pub attributes: A,
    pub value: T,
}

impl<T: Serialize, A: Serialize> Serialize for WithAttributes<T, A> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("$__yson_attributes", 2)?;
        state.serialize_field("$attributes", &self.attributes)?;
        state.serialize_field("$value", &self.value)?;
        state.end()
    }
}

impl<'de, T, A> Deserialize<'de> for WithAttributes<T, A>
where
    T: Deserialize<'de>,
    A: Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct WAVisitor<T, A>(PhantomData<(T, A)>);

        impl<'de, T, A> Visitor<'de> for WAVisitor<T, A>
        where
            T: Deserialize<'de>,
            A: Deserialize<'de>,
        {
            type Value = WithAttributes<T, A>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("WithAttributes struct")
            }

            fn visit_seq<V: SeqAccess<'de>>(self, mut seq: V) -> Result<Self::Value, V::Error> {
                let attributes = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let value = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;

                Ok(WithAttributes { attributes, value })
            }
        }

        deserializer.deserialize_struct(
            "$__yson_attributes",
            &["$attributes", "$value"],
            WAVisitor(PhantomData),
        )
    }
}

impl<V, A> Deref for WithAttributes<V, A> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<V, A> DerefMut for WithAttributes<V, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}
