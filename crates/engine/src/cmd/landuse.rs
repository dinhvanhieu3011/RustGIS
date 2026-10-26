//! Circular 12 land-use layers (Thông tư 12/BXD): the code field, codes from names in words, the
//! style and a feature template per land-use group ([`rustgis_map::tt12`]).

use rustgis_data::{FieldType, GeometryType, Value};
use rustgis_map::design::FeatureTemplate;
use rustgis_map::tt12;
use serde_json::{Value as Json, json};

use super::{need_feature_layer, ps};
use crate::{CmdResult, CommandSpec as C, Session};

pub fn commands() -> Vec<C> {
    vec![
        C::new("landuse.tt12", "Circular 12 Land Use", "Data › Data Design › Fields", tt12_layer)
            .when(need_feature_layer)
            .params(r#"{"layer"?, "nameField"?: field of land-use names in words}"#),
    ]
}

/// Makes a polygon layer a Circular 12 land-use layer. Codes already in `layer` stay; an empty
/// one takes the group whose name `nameField` holds, or stays empty when none matches.
fn tt12_layer(s: &mut Session, p: &Json) -> CmdResult {
    let id = s.layer_id(p)?;
    let l = s.map_mut().layer_mut(id).ok_or("no such layer")?;
    if l.geometry_type() != GeometryType::Polygon {
        return Err("Circular 12 styles polygon layers".into());
    }
    if l.dataset.field_index(tt12::FIELD).and_then(|i| l.dataset.fields.get(i)).is_some_and(|f| f.ty != FieldType::Text) {
        return Err(format!("the field '{}' isn't text", tt12::FIELD));
    }
    let names = match ps(p, "nameField") {
        Some(n) => Some(l.dataset.field_index(n).ok_or_else(|| format!("no field named '{n}'"))?),
        None => None,
    };
    let code = l.dataset.ensure_field(tt12::FIELD, FieldType::Text);
    let (mut matched, mut unmatched, mut kept) = (0, 0, 0);
    for r in 0..l.dataset.features.len() {
        let has = |v: &Value| matches!(v, Value::Text(t) if !t.trim().is_empty());
        if has(l.dataset.value(r, code)) {
            kept += 1;
            continue;
        }
        let Some(fi) = names else { continue };
        let name = match l.dataset.value(r, fi) {
            Value::Text(t) if !t.trim().is_empty() => t.clone(),
            _ => continue,
        };
        match tt12::group_by_name(&name) {
            Some(g) => {
                if let Some(v) = l.dataset.features.get_mut(r).and_then(|f| f.attrs.get_mut(code)) {
                    *v = Value::Text(g.code.into());
                    matched += 1;
                }
            }
            None => unmatched += 1,
        }
    }
    l.renderer = tt12::renderer();
    l.tt12 = true;
    let ours = |name: &str| tt12::GROUPS.iter().any(|g| name.ends_with(&format!(" ({})", g.code)));
    l.design.templates.retain(|t| !ours(&t.name));
    for g in &tt12::GROUPS {
        l.design.templates.push(FeatureTemplate {
            name: format!("{} ({})", g.name, g.code),
            description: String::new(),
            defaults: vec![(tt12::FIELD.into(), Value::Text(g.code.into()))],
        });
    }
    Ok(json!({"matched": matched, "unmatched": unmatched, "kept": kept, "templates": tt12::GROUPS.len()}))
}
