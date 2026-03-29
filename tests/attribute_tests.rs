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

    let mut serializer = Serializer::new(false); // Text mode
    data.serialize(&mut serializer).unwrap();
    let result = String::from_utf8(serializer.output).unwrap();

    assert!(result.starts_with('<'));
    assert!(result.contains("active=%true"));
    assert!(result.contains("role=admin"));
    assert!(result.contains(">{"));
    assert!(result.contains("name=Alice"));
    assert!(result.contains("age=30u"));
    assert!(result.ends_with('}'));
}

#[test]
fn test_deserialize_with_attributes_text() {
    let input = b"<\"active\"=%true; \"role\"=\"admin\">{\"name\"=\"Alice\"; \"age\"=30u}";

    let mut deserializer = Deserializer::from_bytes(input, false);
    let result: WithAttributes<User, Meta> =
        WithAttributes::deserialize(&mut deserializer).unwrap();

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
