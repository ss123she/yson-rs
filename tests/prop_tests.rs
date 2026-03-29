use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use yson::{de::Deserializer, ser::Serializer};

fn roundtrip<T>(value: &T, is_binary: bool) -> T
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let mut serializer = Serializer::new(is_binary);
    value.serialize(&mut serializer).unwrap();

    let mut deserializer = Deserializer::from_bytes(&serializer.output, is_binary);
    T::deserialize(&mut deserializer).unwrap()
}

proptest! {
    #[test]
    fn prop_roundtrip_i64(v in any::<i64>()) {
        assert_eq!(v, roundtrip(&v, true));
        assert_eq!(v, roundtrip(&v, false));
    }

    #[test]
    fn prop_roundtrip_f64(v in any::<f64>()) {
        if !v.is_nan() {
            assert_eq!(v, roundtrip(&v, true));
            assert_eq!(v, roundtrip(&v, false));
        }
    }

    #[test]
    fn prop_roundtrip_string(v in "\\PC*") {
        assert_eq!(v, roundtrip(&v, true));
        assert_eq!(v, roundtrip(&v, false));
    }

    #[test]
    fn prop_roundtrip_complex(
        v in proptest::collection::btree_map(
            "[a-zA-Z0-9_]+",
            proptest::collection::vec(any::<i32>(), 0..50),
            0..20
        )
    ) {
        assert_eq!(v, roundtrip(&v, true));
        assert_eq!(v, roundtrip(&v, false));
    }
}
