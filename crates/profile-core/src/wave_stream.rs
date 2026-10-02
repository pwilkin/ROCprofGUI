use crate::Token;
use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;

pub(crate) fn read(
    source: impl std::io::Read,
    cancelled: &std::sync::atomic::AtomicBool,
    mut accept: impl FnMut(Token) -> Result<(), String>,
) -> Result<Map<String, Value>, String> {
    let reader = std::io::BufReader::with_capacity(
        64 * 1024,
        crate::cancellable_read::CancellableRead::with_message(
            source,
            cancelled,
            "Wave query cancelled",
        ),
    );
    let mut decoder = serde_json::Deserializer::from_reader(reader);
    let fields = ObjectSeed {
        accept: &mut accept,
        body: false,
    }
    .deserialize(&mut decoder)
    .map_err(|e| e.to_string())?;
    decoder.end().map_err(|e| e.to_string())?;
    Ok(fields)
}

struct ObjectSeed<'a, F> {
    accept: &'a mut F,
    body: bool,
}
impl<'de, F: FnMut(Token) -> Result<(), String>> DeserializeSeed<'de> for ObjectSeed<'_, F> {
    type Value = Map<String, Value>;
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_map(self)
    }
}
impl<'de, F: FnMut(Token) -> Result<(), String>> Visitor<'de> for ObjectSeed<'_, F> {
    type Value = Map<String, Value>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a wave object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut fields = Map::new();
        let mut found = false;
        while let Some(key) = map.next_key::<String>()? {
            if !self.body && key == "wave" {
                if found {
                    return Err(A::Error::duplicate_field("wave"));
                }
                found = true;
                fields = map.next_value_seed(ObjectSeed {
                    accept: self.accept,
                    body: true,
                })?;
            } else if self.body && key == "instructions" {
                if found {
                    return Err(A::Error::duplicate_field("instructions"));
                }
                found = true;
                map.next_value_seed(TokensSeed(self.accept))?;
            } else if self.body {
                fields.insert(key, map.next_value()?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        if !self.body && !found {
            return Err(A::Error::missing_field("wave"));
        }
        Ok(fields)
    }
}

struct TokensSeed<'a, F>(&'a mut F);
impl<'de, F: FnMut(Token) -> Result<(), String>> DeserializeSeed<'de> for TokensSeed<'_, F> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_any(self)
    }
}
impl<'de, F: FnMut(Token) -> Result<(), String>> Visitor<'de> for TokensSeed<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an instruction token array or null")
    }
    fn visit_unit<E: Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let cap = crate::caps::max_records();
        for _ in 0..cap {
            let Some(row) = seq.next_element::<Value>()? else {
                return Ok(());
            };
            (self.0)(crate::import::parse_wave_token(&row).map_err(A::Error::custom)?)
                .map_err(A::Error::custom)?;
        }
        // A seed that errors on sight rejects an over-cap input BEFORE parsing the
        // extra element, and carries the effective cap into the message (rather
        // than a hard-coded "two million" a raised/lowered cap would belie).
        seq.next_element_seed(ExcessToken(cap))?;
        Ok(())
    }
}
struct ExcessToken(usize);
impl<'de> DeserializeSeed<'de> for ExcessToken {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, _: D) -> Result<(), D::Error> {
        Err(D::Error::custom(format!("Input exceeds the {} row limit", self.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::read;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn document_shape_and_sink_failures_do_not_return_partial_results() {
        for source in [
            r#"{"wave":{"begin":0,"end":1}}"#,
            r#"{"wave":{"instructions":null}}"#,
            r#"{"wave":{"instructions":[]}}"#,
        ] {
            read(source.as_bytes(), &AtomicBool::new(false), |_| {
                panic!("unexpected token")
            })
            .unwrap();
        }
        for source in [
            r#"{}"#,
            r#"{"wave":null}"#,
            r#"{"wave":{"instructions":{}}}"#,
            r#"{"wave":{"instructions":[],"instructions":[]}}"#,
            r#"{"wave":{},"wave":{}}"#,
            r#"{"wave":{}} false"#,
        ] {
            assert!(
                read(source.as_bytes(), &AtomicBool::new(false), |_| Ok(())).is_err(),
                "{source}"
            );
        }
        let mut calls = 0;
        let error = read(
            br#"{"wave":{"instructions":[[0,6,0,1,2],[1,6,0,1,2],[2,6,0,1,2]]}}"#.as_slice(),
            &AtomicBool::new(false),
            |_| {
                calls += 1;
                Err("Wave query cancelled".into())
            },
        )
        .unwrap_err();
        assert!(error.contains("Wave query cancelled"));
        assert_eq!(calls, 1);
    }

    #[test]
    #[ignore = "explicit two-million-row streaming boundary qualification"]
    fn streaming_limit_counts_discarded_rows_and_rejects_extra_before_parsing() {
        let mut source = String::from("{\"wave\":{\"instructions\":[");
        for i in 0..crate::import::MAX_ROWS {
            if i > 0 {
                source.push(',');
            }
            source.push_str("[0,6,0,1,2]");
        }
        source.push_str("]}}");
        let mut count = 0;
        read(source.as_bytes(), &AtomicBool::new(false), |_| {
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, crate::import::MAX_ROWS);
        source.truncate(source.len() - 3);
        source.push_str(",[");
        let error = read(source.as_bytes(), &AtomicBool::new(false), |_| Ok(())).unwrap_err();
        assert!(error.contains(&format!("{} row limit", crate::import::MAX_ROWS)), "{error}");
    }
}
