//! Digitising CAD drawings: tools over the layers a DXF or DWG drawing is read into
//! (`rustgis_data::dxf`), which keep each entity's CAD layer in the `layer` field.

use std::collections::HashMap;

use rustgis_data::{Dataset, FieldType, Value};
use serde_json::Value as Json;

use crate::{Inputs, Output, OutputLayer, ToolResult};

/// The most layers one run makes (a hostile drawing can have thousands of CAD layers).
pub const MAX_OUTPUTS: usize = 500;

/// Whether a CAD layer name matches a pattern: ASCII case-insensitive, `*` any run of non-space characters.
pub fn matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.trim().chars().map(|c| c.to_ascii_lowercase()).collect();
    let n: Vec<char> = name.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut i, mut j) = (0, 0);
    // The last `*` seen and where in the name it started matching: backtracked to on a mismatch.
    let mut star: Option<(usize, usize)> = None;
    while let Some(&c) = n.get(j) {
        match p.get(i) {
            Some('*') => {
                star = Some((i, j));
                i += 1;
            }
            Some(&pc) if pc == c => {
                i += 1;
                j += 1;
            }
            _ => match star {
                Some((si, sj)) => {
                    // When backtracking through a *, don't skip spaces
                    if c == ' ' {
                        return false;
                    }
                    star = Some((si, sj + 1));
                    i = si + 1;
                    j = sj + 1;
                }
                None => return false,
            },
        }
    }
    p.get(i..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

/// `cadLayers`: one layer per CAD layer (or per prefix before the first `_`), keeping the CAD
/// layers `layers` names; a text field whose values are all numbers, read with a decimal comma
/// when `decimalComma`, becomes a number field.
pub fn split_layers(inp: &dyn Inputs, p: &Json) -> Result<ToolResult, String> {
    let text = |k: &str| p.get(k).and_then(Json::as_str).unwrap_or("");
    let name = text("input");
    let ds = inp.dataset(name).ok_or_else(|| format!("no layer named '{name}'"))?;
    let li = ds.field_index("layer").ok_or("the input has no 'layer' field: split a layer read from a DXF or DWG drawing")?;
    let patterns: Vec<&str> = text("layers").split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
    let by_prefix = text("group").eq_ignore_ascii_case("Prefix");
    let comma = p.get("decimalComma").and_then(Json::as_bool).unwrap_or(true);
    let selected = inp.selection(name);
    let rows: Vec<usize> = if selected.is_empty() {
        (0..ds.features.len()).collect()
    } else {
        selected.iter().copied().filter(|r| *r < ds.features.len()).collect()
    };

    // Groups in the order first seen: their name as first written, and their rows.
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut over: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in rows {
        let cad = match ds.value(r, li) {
            Value::Text(t) => t.trim(),
            _ => "0",
        };
        if !patterns.is_empty() && !patterns.iter().any(|pt| matches(pt, cad)) {
            continue;
        }
        let label = if by_prefix { cad.split('_').next().unwrap_or(cad) } else { cad };
        let key = label.to_ascii_lowercase();
        match index.get(&key) {
            Some(&g) => {
                if let Some((_, rs)) = groups.get_mut(g) {
                    rs.push(r);
                }
            }
            None if groups.len() >= MAX_OUTPUTS => {
                over.insert(key);
            }
            None => {
                index.insert(key, groups.len());
                groups.push((label.to_string(), vec![r]));
            }
        }
    }
    if groups.is_empty() {
        return Err(if patterns.is_empty() {
            format!("'{name}' has no features")
        } else {
            format!("no CAD layer matches '{}'", text("layers"))
        });
    }
    let mut messages = Vec::new();
    if !over.is_empty() {
        messages.push(format!("{} more CAD layers were left out: at most {MAX_OUTPUTS} layers are made", over.len()));
    }
    let outs = groups.into_iter().map(|(label, rs)| OutputLayer::Vector(subset(ds, &rs, label, comma))).collect();
    Ok(ToolResult { output: Output::Layers(outs), messages })
}

/// A value read as a number: `,` taken as the decimal point when `comma`.
fn number(v: &Value, comma: bool) -> Option<f64> {
    let Value::Text(t) = v else { return None };
    let t = t.trim();
    let t = if comma { t.replace(',', ".") } else { t.to_string() };
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

fn blank(v: &Value) -> bool {
    matches!(v, Value::Null) || matches!(v, Value::Text(t) if t.trim().is_empty())
}

/// The rows of `ds` as a new layer: the fields holding a value in them, text fields of numbers
/// made number fields.
fn subset(ds: &Dataset, rows: &[usize], name: String, comma: bool) -> Dataset {
    let keep: Vec<usize> = (0..ds.fields.len()).filter(|&f| rows.iter().any(|&r| !blank(ds.value(r, f)))).collect();
    let mut fields = Vec::new();
    let mut numeric = Vec::new();
    for &f in &keep {
        let Some(field) = ds.fields.get(f) else { continue };
        let mut field = field.clone();
        let is_number =
            field.ty == FieldType::Text && rows.iter().map(|&r| ds.value(r, f)).filter(|v| !blank(v)).all(|v| number(v, comma).is_some());
        if is_number {
            field.ty = FieldType::Double;
        }
        fields.push(field);
        numeric.push(is_number);
    }
    let mut out = Dataset::new(name, ds.geometry_type, fields);
    out.crs = ds.crs;
    for &r in rows {
        let Some(f) = ds.features.get(r) else { continue };
        let attrs = keep
            .iter()
            .zip(&numeric)
            .map(|(&i, &n)| {
                let v = f.attrs.get(i).cloned().unwrap_or_default();
                if n { number(&v, comma).map(Value::Float).unwrap_or_default() } else { v }
            })
            .collect();
        out.push_next(f.geometry.clone(), attrs);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustgis_data::{Coord, Field, Geometry, GeometryType};

    struct One(Dataset);
    impl Inputs for One {
        fn dataset(&self, layer: &str) -> Option<&Dataset> {
            (layer == self.0.name).then_some(&self.0)
        }
        fn selection(&self, _: &str) -> &[usize] {
            &[]
        }
    }

    fn vectors(r: ToolResult) -> Vec<Dataset> {
        match r.output {
            Output::Layers(v) => v.into_iter().filter_map(|o| if let OutputLayer::Vector(d) = o { Some(d) } else { None }).collect(),
            _ => panic!("expected layers"),
        }
    }

    fn drawing() -> One {
        let mut d = Dataset::new("pts", GeometryType::Point, vec![Field::new("layer", FieldType::Text), Field::new("TK", FieldType::Text)]);
        for (i, (layer, tk)) in
            [("ONG 300", "3,43"), ("ONG 500", "2,5"), ("DAT_CT_Dichvu", ""), ("DAT_Anninh", ""), ("ong 300", "1")].iter().enumerate()
        {
            d.push(Some(Geometry::Point(Coord::new(i as f64, 0.0))), vec![Value::Text((*layer).into()), Value::Text((*tk).into())]);
        }
        One(d)
    }

    #[test]
    fn one_layer_per_cad_layer() {
        let outs = vectors(split_layers(&drawing(), &serde_json::json!({"input": "pts"})).unwrap());
        let names: Vec<&str> = outs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["ONG 300", "ONG 500", "DAT_CT_Dichvu", "DAT_Anninh"], "case-insensitive, first spelling kept");
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].fields[tk].ty, FieldType::Double);
        assert_eq!(outs[0].value(0, tk), &Value::Float(3.43));
        assert_eq!(outs[0].value(1, tk), &Value::Float(1.0));
        assert!(outs[2].field_index("TK").is_none(), "a field empty in the group is left out");
    }

    #[test]
    fn patterns_prefixes_and_commas() {
        let outs = vectors(split_layers(&drawing(), &serde_json::json!({"input": "pts", "layers": "ONG *"})).unwrap());
        assert_eq!(outs.len(), 2);
        let outs = vectors(split_layers(&drawing(), &serde_json::json!({"input": "pts", "layers": "DAT_*", "group": "Prefix"})).unwrap());
        assert_eq!(outs.len(), 1);
        assert_eq!((outs[0].name.as_str(), outs[0].features.len()), ("DAT", 2));
        let outs = vectors(split_layers(&drawing(), &serde_json::json!({"input": "pts", "decimalComma": false})).unwrap());
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].fields[tk].ty, FieldType::Text);
        assert!(split_layers(&drawing(), &serde_json::json!({"input": "pts", "layers": "NOPE"})).is_err());
    }

    #[test]
    fn mixed_columns_stay_text_and_layer_field_is_required() {
        let mut d = drawing();
        d.0.features[1].attrs[1] = Value::Text("A1".into());
        d.0.features[1].attrs[0] = Value::Text("ONG 300".into());
        let outs = vectors(split_layers(&d, &serde_json::json!({"input": "pts"})).unwrap());
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].fields[tk].ty, FieldType::Text, "one value that isn't a number keeps the column text");
        let plain = One(Dataset::new("pts", GeometryType::Point, vec![Field::new("v", FieldType::Text)]));
        assert!(split_layers(&plain, &serde_json::json!({"input": "pts"})).unwrap_err().contains("'layer'"));
    }

    #[test]
    fn outputs_are_bounded() {
        let mut d = Dataset::new("pts", GeometryType::Point, vec![Field::new("layer", FieldType::Text)]);
        for i in 0..MAX_OUTPUTS + 100 {
            d.push(Some(Geometry::Point(Coord::new(0.0, 0.0))), vec![Value::Text(format!("L{i}"))]);
        }
        let r = split_layers(&One(d), &serde_json::json!({"input": "pts"})).unwrap();
        assert!(r.messages.iter().any(|m| m.contains("100 more")), "{:?}", r.messages);
        assert_eq!(vectors(r).len(), MAX_OUTPUTS);
    }

    /// From the reader to the tool: a manhole's level, an attribute written with a decimal comma,
    /// comes out a number.
    #[test]
    fn manhole_levels_become_numbers() {
        let text = "  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\nT-HO GA\n 66\n1\n  2\nHG\n 10\n1\n 20\n2\n  0\nATTRIB\n 10\n1\n 20\n2\n  2\nTK\n  1\n3,43\n  0\nSEQEND\n  0\nENDSEC\n  0\nEOF\n";
        let read = rustgis_data::dxf::read("a.dxf", text.as_bytes(), &rustgis_data::dxf::Options::default()).unwrap();
        let mut pts = read.into_iter().next().unwrap().0;
        pts.name = "pts".into();
        let outs = vectors(split_layers(&One(pts), &serde_json::json!({"input": "pts"})).unwrap());
        assert_eq!(outs.len(), 1);
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].value(0, tk), &Value::Float(3.43));
    }

    #[test]
    fn patterns() {
        assert!(matches("ONG *", "ONG 300"));
        assert!(matches("ong *", "ONG 500"));
        assert!(matches("DAT_*", "DAT_CTSX_Congnghiep"));
        assert!(matches("*_Songsuoi", "QHDH_DAT_KHAC_Songsuoi"));
        assert!(matches("D*T*", "DAT_Anninh"));
        assert!(matches("*", ""));
        assert!(matches("Chialo", "chialo"));
        assert!(!matches("ONG *", "ong tnm B1000"));
        assert!(!matches("DAT_*", "QHDH_DAT_KHAC_Songsuoi"));
        assert!(!matches("Chialo", "Chialo2"));
        assert!(!matches("", "x"));
        assert!(matches(" ONG * ", "ONG 300"), "spaces around a pattern don't count");
    }
}
