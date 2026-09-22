use crate::{RealtimeAnchor, RealtimeCalibration};
use serde_json::Value;

pub fn parse(value: &Value) -> Result<RealtimeCalibration, String> {
    let object = value
        .as_object()
        .ok_or("REALTIME sidecar must be an object")?;
    let frequency_hz = match value.get("metadata").and_then(|v| v.get("frequency")) {
        None | Some(Value::Null) => None,
        Some(value) => match value
            .as_u64()
            .ok_or("REALTIME frequency must be a nonnegative integer")?
        {
            0 => None,
            frequency => Some(frequency.to_string()),
        },
    };
    let mut anchors = Vec::new();
    for (key, value) in object {
        let Some(suffix) = key.strip_prefix("SE") else {
            continue;
        };
        if suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_digit()) {
            return Err("Invalid REALTIME shader-engine key".into());
        }
        let se = suffix
            .parse::<u32>()
            .map_err(|_| "REALTIME shader-engine ID exceeds u32")?;
        let points = value
            .as_array()
            .ok_or("REALTIME engine samples must be an array")?;
        if anchors.len() + points.len() > 100_000 {
            return Err("REALTIME sidecar exceeds 100000 anchors".into());
        }
        for point in points {
            let pair = point
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or("REALTIME sample must be a clock pair")?;
            anchors.push(RealtimeAnchor {
                se,
                shader_clock: pair[0]
                    .as_i64()
                    .ok_or("REALTIME shader clock must be an i64 integer")?
                    .to_string(),
                realtime_clock: pair[1]
                    .as_u64()
                    .ok_or("REALTIME clock must be a u64 integer")?
                    .to_string(),
            });
        }
    }
    Ok(RealtimeCalibration {
        frequency_hz,
        anchors,
    })
}
