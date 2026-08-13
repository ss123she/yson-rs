#![cfg(feature = "serde")]

use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use yson_rs::{Deserializer, Serializer, WithAttributes, YsonFormat};

use crate::common::*;

mod common;

fn roundtrip<T>(value: &T, format: YsonFormat) -> T
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let mut serializer = Serializer::new(format);
    value
        .serialize(&mut serializer)
        .expect("Serialization failed");

    let mut deserializer = Deserializer::new(&serializer.output, format);
    T::deserialize(&mut deserializer).expect("Deserialization failed")
}

prop_compose! {
    fn user_strategy()(name in "[a-zA-Z0-9 ]*", age in any::<u32>()) -> User {
        User { name, age }
    }
}

prop_compose! {
    fn meta_strategy()(active in any::<bool>(), role in "[a-zA-Z0-9 ]*") -> Meta {
        Meta { active, role }
    }
}

fn status_strategy() -> impl Strategy<Value = UserStatus> {
    prop_oneof![
        Just(UserStatus::Pending),
        Just(UserStatus::Active),
        "[a-zA-Z0-9 ]*".prop_map(UserStatus::Banned),
        (any::<u32>(), "[a-zA-Z0-9 ]*")
            .prop_map(|(code, reason)| UserStatus::Custom { code, reason })
    ]
}

prop_compose! {
    fn complex_entity_strategy()(
        id in any::<u64>(),
        user_val in user_strategy(),
        user_meta in meta_strategy(),
        tags in proptest::collection::vec("[a-zA-Z0-9 ]*", 0..5),
        status in proptest::option::of(status_strategy())
    ) -> ComplexEntity {
        ComplexEntity {
            id,
            user: WithAttributes { attributes: user_meta, value: user_val },
            tags,
            status,
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn prop_roundtrip_all_primitives(
        b in any::<bool>(),
        u in any::<u64>(),
        i in any::<i64>(),
        s in "[a-zA-Z0-9_]*",
    ) {
        assert_eq!(b, roundtrip(&b, YsonFormat::Binary));
        assert_eq!(u, roundtrip(&u, YsonFormat::Binary));
        assert_eq!(i, roundtrip(&i, YsonFormat::Binary));
        assert_eq!(s, roundtrip(&s, YsonFormat::Binary));

        assert_eq!(b, roundtrip(&b, YsonFormat::Text));
        assert_eq!(u, roundtrip(&u, YsonFormat::Text));
        assert_eq!(i, roundtrip(&i, YsonFormat::Text));
        assert_eq!(s, roundtrip(&s, YsonFormat::Text));
    }

    #[test]
    fn prop_roundtrip_f64(v in any::<f64>()) {
        if !v.is_nan() {
            assert_eq!(v, roundtrip(&v, YsonFormat::Binary));
            assert_eq!(v, roundtrip(&v, YsonFormat::Text));
        }
    }

    #[test]
    fn prop_roundtrip_complex_map(
        v in proptest::collection::hash_map(
            "[a-z]+",
            any::<i32>(),
            0..10
        )
    ) {
        assert_eq!(v, roundtrip(&v, YsonFormat::Binary));
        assert_eq!(v, roundtrip(&v, YsonFormat::Text));
    }

    #[test]
    fn prop_roundtrip_complex_entity(v in complex_entity_strategy()) {
        assert_eq!(v, roundtrip(&v, YsonFormat::Binary));
        assert_eq!(v, roundtrip(&v, YsonFormat::Text));
    }
}
