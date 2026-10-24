//! Strict JSON for everything fetched from the network: duplicate keys and deep nesting are
//! errors (a plain `serde_json::from_slice` keeps the last duplicate silently), and nothing
//! may follow the value.
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::collections::HashSet;
use std::fmt;

/// Deepest container nesting accepted in widget files. serde_json's own limit is 128.
pub const MAX_DEPTH: usize = 32;
/// The index embeds every widget's layout, so it nests a little deeper than any one file.
pub const MAX_DEPTH_INDEX: usize = 64;

struct Seed {
    depth: usize,
    max: usize,
}

impl Seed {
    fn child(&self) -> Seed {
        Seed {
            depth: self.depth + 1,
            max: self.max,
        }
    }
}

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("number out of range"))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        if self.depth >= self.max {
            return Err(de::Error::custom("nested too deeply"));
        }
        let mut out = Vec::new();
        while let Some(v) = a.next_element_seed(self.child())? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        if self.depth >= self.max {
            return Err(de::Error::custom("nested too deeply"));
        }
        let mut seen = HashSet::new();
        let mut out = Map::new();
        while let Some(k) = a.next_key::<String>()? {
            if !seen.insert(k.clone()) {
                // The key is not echoed: it is attacker-chosen text.
                return Err(de::Error::custom("duplicate object key"));
            }
            out.insert(k, a.next_value_seed(self.child())?);
        }
        Ok(Value::Object(out))
    }
}

/// Parse `bytes` as exactly one JSON value. Errors are short fixed-form messages that never
/// quote the input.
pub fn strict(bytes: &[u8], max_depth: usize) -> Result<Value, String> {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let v = Seed {
        depth: 0,
        max: max_depth,
    }
    .deserialize(&mut de)
    .map_err(|e| invalid(&e))?;
    de.end().map_err(|e| invalid(&e))?;
    Ok(v)
}

fn invalid(e: &serde_json::Error) -> String {
    // serde_json messages name a line, a column and a category; the custom ones above are fixed.
    let s = e.to_string();
    format!(
        "not valid JSON ({})",
        s.chars().take(80).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_values_parse() {
        let v = strict(br#"{"a":[1,2.5,"x",null,true],"b":{"c":-3}}"#, MAX_DEPTH).unwrap();
        assert_eq!(v["a"][1], 2.5);
        assert_eq!(v["b"]["c"], -3);
    }

    #[test]
    fn duplicate_keys_are_errors_at_any_level() {
        for s in [
            r#"{"a":1,"a":2}"#,
            r#"{"x":{"a":1,"a":1}}"#,
            r#"[{"a":1,"b":2,"a":3}]"#,
            r#"{"id":"x","id":"y"}"#,
        ] {
            let e = strict(s.as_bytes(), MAX_DEPTH).unwrap_err();
            assert!(e.contains("duplicate object key"), "{s}: {e}");
            assert!(!e.contains('x') || !e.contains("\"x\""), "key echoed: {e}");
        }
    }

    #[test]
    fn deep_nesting_is_refused_at_the_limit() {
        let nest = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
        assert!(strict(nest(MAX_DEPTH).as_bytes(), MAX_DEPTH).is_ok());
        assert!(strict(nest(MAX_DEPTH + 1).as_bytes(), MAX_DEPTH).is_err());
        let obj = |n: usize| format!("{}1{}", "{\"a\":".repeat(n), "}".repeat(n));
        assert!(strict(obj(MAX_DEPTH).as_bytes(), MAX_DEPTH).is_ok());
        assert!(strict(obj(MAX_DEPTH + 1).as_bytes(), MAX_DEPTH).is_err());
        // Far past serde_json's own recursion limit: an error, not a stack overflow.
        assert!(strict(nest(100_000).as_bytes(), MAX_DEPTH).is_err());
    }

    #[test]
    fn trailing_data_bom_and_bad_utf8_are_errors() {
        for b in [
            &b"{} {}"[..],
            b"{}x",
            b"\xef\xbb\xbf{}",
            b"{\"a\":\"\xff\"}",
            b"",
            b"   ",
            b"{\"a\":1,}",
            b"NaN",
            b"{\"a\":1e999}",
        ] {
            assert!(
                strict(b, MAX_DEPTH).is_err(),
                "{:?}",
                String::from_utf8_lossy(b)
            );
        }
        // Trailing whitespace is fine.
        assert!(strict(b"{}\n", MAX_DEPTH).is_ok());
    }

    #[test]
    fn errors_do_not_quote_the_input() {
        let e = strict(br#"{"SECRET-MARKER": tru}"#, MAX_DEPTH).unwrap_err();
        assert!(!e.contains("SECRET-MARKER"), "{e}");
    }
}
