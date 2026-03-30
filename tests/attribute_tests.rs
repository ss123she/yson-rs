use serde::{Deserialize, Serialize};
use yson::{attributes::WithAttributes, de::Deserializer, ser::Serializer};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct User {
    name: String,
    age: u32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct Meta {
    active: bool,
    role: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
enum UserStatus {
    Pending,
    Active,
    Banned(String),
    Custom { code: u32, reason: String },
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct ComplexEntity {
    id: u64,
    user: WithAttributes<User, Meta>,
    tags: Vec<String>,
    status: Option<UserStatus>,
}

fn roundtrip<T>(value: &T, is_binary: bool) -> T
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let mut serializer = Serializer::new(is_binary);
    value
        .serialize(&mut serializer)
        .expect("Serialization failed");

    let mut deserializer = Deserializer::from_bytes(&serializer.output, is_binary);
    T::deserialize(&mut deserializer).expect("Deserialization failed")
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_serialize_with_attributes_text() {
        let data = WithAttributes {
            attributes: Meta {
                active: true,
                role: "admin".to_string(),
            },
            value: User {
                name: "Alice".to_string(),
                age: 30,
            },
        };

        let mut serializer = Serializer::new(false);
        data.serialize(&mut serializer).unwrap();
        let result = String::from_utf8(serializer.output).unwrap();

        assert!(result.starts_with('<'));
        assert!(result.contains("active=%true"));
        assert!(result.contains("role=admin"));
        assert!(result.contains(">"));
        assert!(result.contains("name=Alice"));
        assert!(result.contains("age=30u"));
    }

    #[test]
    fn test_deserialize_with_attributes_text() {
        let input = b"<\"active\"=%true; \"role\"=\"admin\">{\"name\"=\"Alice\"; \"age\"=30u}";

        let mut deserializer = Deserializer::from_bytes(input, false);
        let result: WithAttributes<User, Meta> =
            WithAttributes::deserialize(&mut deserializer).expect("Failed to deserialize");

        assert!(result.attributes.active);
        assert_eq!(result.attributes.role, "admin");
        assert_eq!(result.value.name, "Alice");
        assert_eq!(result.value.age, 30);
    }

    #[test]
    fn test_deserialize_fallback_without_attributes() {
        let input = b"{name=Bob; age=25u}";

        let mut deserializer = Deserializer::from_bytes(input, false);
        let result: WithAttributes<User, Option<Meta>> =
            WithAttributes::deserialize(&mut deserializer).unwrap();

        assert!(result.attributes.is_none());
        assert_eq!(result.value.name, "Bob");
        assert_eq!(result.value.age, 25);
    }

    #[test]
    fn test_serialize_deserialize_binary_mode() {
        let data = WithAttributes {
            attributes: Meta {
                active: true,
                role: "superuser".to_string(),
            },
            value: User {
                name: "Dave".to_string(),
                age: 40,
            },
        };

        let result = roundtrip(&data, true);
        assert_eq!(data, result);
    }

    #[test]
    fn test_nested_with_attributes() {
        let nested = WithAttributes {
            attributes: Meta {
                active: true,
                role: "outer".to_string(),
            },
            value: WithAttributes {
                attributes: Meta {
                    active: false,
                    role: "inner".to_string(),
                },
                value: 42i64,
            },
        };

        assert_eq!(nested, roundtrip(&nested, false));
        assert_eq!(nested, roundtrip(&nested, true));
    }
}
