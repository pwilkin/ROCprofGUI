use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn instances(definition: &Value) -> Result<BTreeMap<u64, String>, String> {
    let mut result = BTreeMap::new();
    let Some(instances) = definition.get("instances").filter(|value| !value.is_null()) else {
        return Ok(result);
    };
    for instance in instances
        .as_array()
        .ok_or("Invalid SPM instance metadata")?
    {
        let id = instance["instance_id"]
            .as_u64()
            .ok_or("Invalid SPM metadata instance ID")?;
        let dimensions = instance["dimensions"]
            .as_array()
            .ok_or("Invalid SPM dimension metadata")?;
        let mut seen = BTreeSet::new();
        let mut normalized = BTreeMap::new();
        for dimension in dimensions {
            let name = dimension["dimension_name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("Invalid SPM dimension name")?;
            let index = dimension["index"]
                .as_u64()
                .filter(|n| *n <= 9_007_199_254_740_991)
                .ok_or("Invalid SPM dimension index")?;
            if !seen.insert(name) {
                return Err("Duplicate SPM dimension name".into());
            }
            normalized.insert(name, index);
        }
        let serialized = serde_json::to_string(
            &normalized
                .into_iter()
                .map(|(name, index)| json!({"dimension_name":name,"index":index}))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?;
        if let Some(previous) = result.insert(id, serialized.clone()) {
            if previous != serialized {
                return Err("Conflicting SPM instance dimensions".into());
            }
        }
    }
    Ok(result)
}

pub fn packed(id: u64) -> Result<String, String> {
    if (id >> 46) & 3 != 0 {
        return Err("SPM instance ID uses reserved coordinate bits; explicit dimension metadata is required".into());
    }
    let layout = [
        ("DIMENSION_XCC", 0, 63, true),
        ("DIMENSION_AID", 6, 63, false),
        ("DIMENSION_SHADER_ENGINE", 12, 63, true),
        ("DIMENSION_AGENT", 18, 63, false),
        ("DIMENSION_SHADER_ARRAY", 24, 63, false),
        ("DIMENSION_WGP", 30, 63, false),
        ("DIMENSION_INSTANCE", 36, 1023, true),
    ];
    serde_json::to_string(
        &layout
            .into_iter()
            .filter_map(|(name, shift, mask, required)| {
                let index = (id >> shift) & mask;
                (required || index != 0).then(|| json!({"dimension_name":name,"index":index}))
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())
}
