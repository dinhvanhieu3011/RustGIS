# CAD Layers (phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Phase 1 of `docs/superpowers/specs/2026-10-28-cad-digitising-design.md`: a DXF/DWG drawing's block attributes become fields, its external references are named in the read report, and a `cadLayers` tool splits a drawing's layer into one layer per CAD layer (or name prefix), reading decimal commas.

**Architecture:** Two changes in `crates/data/src/dxf.rs` (the reader sees ATTRIB order and BLOCK flags; nothing else can), one new module `crates/analysis/src/cad.rs` holding the tool, registered in `TOOLS` (`crates/analysis/src/tools.rs`). The engine gets one new struct field in its `dxf::Options` literal. No new dependencies.

**Tech Stack:** Rust (edition with let-chains), `rustgis-data`, `rustgis-analysis`, `serde_json`. Tests are `#[test]` in the same files (`cargo test`).

**Rules from AGENTS.md that bite here:** no `unwrap`/`expect`/`panic!`/indexing on hostile data outside tests (crate roots deny them); bound every count from the file; nothing below `apps/` may break `wasm32-unknown-unknown`; `cargo xtask ci` before every commit.

---

## File map
| File | Change |
|---|---|
| `crates/data/src/dxf.rs` | `Drawing.xrefs`; `Insert.attribs`; `Options::attribs_as_points`; `drawing()` takes the flag; `emit()` places an attribute point; `Out` counts xrefs and dropped tags; report messages; module doc; 3 tests |
| `crates/engine/src/cmd/layer.rs:909` | add `attribs_as_points: false` to the `Options` literal |
| `docs/user-guide/data-formats.md:31` | DXF row: attributes as fields, external references |
| `crates/analysis/src/cad.rs` (new) | `matches`, `split_layers`, `MAX_OUTPUTS`, tests |
| `crates/analysis/src/lib.rs:13` | `pub mod cad;` |
| `crates/analysis/src/tools.rs:538` | `cadLayers` `ToolSpec` before `];` |

---

### Task 1: External references named in the read report

**Files:**
- Modify: `crates/data/src/dxf.rs` (struct `Drawing` ~line 937, fn `drawing` ~949, struct `Out` ~1063, fn `emit` missing-block branch ~1157, fn `read` ~1340–1370)
- Test: `crates/data/src/dxf.rs` (`mod tests`)

Today a BLOCK with flag 70 bit 4 (an external reference) is stored as an empty block, so its INSERTs silently draw nothing.

- [ ] **Step 1: Write the failing test** (add at the end of `mod tests`, before its closing `}`)

```rust
    /// An external reference (BLOCK flag 4, its file in group 1) isn't read: the report names the
    /// file, once however many INSERTs place it, and doesn't count it as a broken block.
    #[test]
    fn external_references_are_reported() {
        let d = dxf(&[
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (2, "Xr.nen"),
            (70, "4"),
            (1, ".\\Xr.nen.HIENTRANG.dwg"),
            (10, "0"),
            (20, "0"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "INSERT"),
            (2, "Xr.nen"),
            (10, "0"),
            (20, "0"),
            (0, "INSERT"),
            (2, "XR.NEN"),
            (10, "5"),
            (20, "5"),
            (0, "POINT"),
            (10, "1"),
            (20, "1"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let l = read("x.dxf", &d, &Options::default()).unwrap();
        let msgs = &l[0].1.messages;
        assert!(msgs.iter().any(|m| m.contains("external drawings not read (1)") && m.contains("Xr.nen.HIENTRANG.dwg")), "{msgs:?}");
        assert_eq!(l[0].1.skipped, 0, "an external reference isn't a broken block");
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p rustgis-data --lib dxf::tests::external_references_are_reported`
Expected: FAIL on the `external drawings not read` assertion.

- [ ] **Step 3: Implement**

In `struct Drawing`, after `blocks`:

```rust
    /// External references: lower-cased block name → the file it names (group 1, else the name).
    xrefs: HashMap<String, String>,
```

In `fn drawing`, the initialiser becomes:

```rust
    let mut d = Drawing {
        layers: HashMap::new(),
        blocks: HashMap::new(),
        xrefs: HashMap::new(),
        entities: Vec::new(),
        unread: BTreeMap::new(),
        bad: 0,
    };
```

The block being read carries its external file. Replace the `block` declaration and the BLOCK / ENDBLK arms:

```rust
    // The block being read: name, base point, entities, and the file it names when it is an
    // external reference.
    let mut block: Option<(String, P3, Vec<Ent>, Option<String>)> = None;
```

```rust
                    if r.kind == "BLOCK" {
                        let name = g.s(2).map(str::trim).unwrap_or("").to_string();
                        let xref = (g.i(70).unwrap_or(0) & 4 != 0)
                            .then(|| g.s(1).map(str::trim).filter(|p| !p.is_empty()).unwrap_or(&name).to_string());
                        block = Some((name, g.pt(10).unwrap_or([0.0; 3]), Vec::new(), xref));
                        continue;
                    }
                    if r.kind == "ENDBLK" {
                        if let Some((name, base, ents, xref)) = block.take()
                            && !name.is_empty()
                        {
                            match xref {
                                Some(file) => {
                                    d.xrefs.insert(name.to_ascii_lowercase(), file);
                                }
                                None => {
                                    d.blocks.insert(name.to_ascii_lowercase(), (base, ents));
                                }
                            }
                        }
                        continue;
                    }
```

and the push at the end of the loop (`Some((_, _, ents)) if section == "BLOCKS"`) becomes `Some((_, _, ents, _)) if section == "BLOCKS"`.

In `struct Out`, add:

```rust
    /// External references placed: lower-cased block name → INSERTs.
    xrefs: BTreeMap<String, usize>,
```

In `fn emit`, inside `let Some((base, inner)) = d.blocks.get(&key) else {`, as its first statement:

```rust
                if d.xrefs.contains_key(&key) {
                    *out.xrefs.entry(key).or_insert(0) += 1;
                    continue;
                }
```

In `fn read`, the `Out` literal gets `xrefs: BTreeMap::new(),`, and after the `out.overworked` message:

```rust
    if !out.xrefs.is_empty() {
        let files: Vec<&str> = out
            .xrefs
            .keys()
            .filter_map(|k| d.xrefs.get(k))
            .map(|f| f.rsplit(['\\', '/']).next().unwrap_or(f))
            .collect();
        report.messages.push(format!(
            "external drawings not read ({}): {}; bind them in CAD (XREF › Bind) to include them",
            files.len(),
            files.join(", ")
        ));
    }
```

- [ ] **Step 4: Run the dxf tests**

Run: `cargo test -p rustgis-data --lib dxf::`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/data/src/dxf.rs
git commit -m "feat(dxf): name external references in the read report

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Block attributes become fields

**Files:**
- Modify: `crates/data/src/dxf.rs` (module doc lines 3–6; `Options` ~line 61; `Insert` ~246; `entity()` INSERT and DIMENSION constructors ~652, ~668; `drawing()`; `Out`; `emit()`; `read()`; `layers()`)
- Modify: `crates/engine/src/cmd/layer.rs:909-913`
- Test: `crates/data/src/dxf.rs` (`mod tests`)

The ATTRIBs between an INSERT (with group 66 = 1) and its SEQEND are attached to that INSERT while reading. When the INSERT is placed, it adds one point at its insertion point carrying `layer`, `entity` = `INSERT`, `block` = the block's name, and one text field per tag.

- [ ] **Step 1: Write the failing tests** (in `mod tests`)

```rust
    /// A manhole block's attributes: one point at the insertion point with a field per tag, not a
    /// point per attribute. A tag named like a built-in field gets `_attr`.
    #[test]
    fn block_attributes_become_fields() {
        let d = dxf(&[
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (2, "HG"),
            (10, "0"),
            (20, "0"),
            (0, "POINT"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "INSERT"),
            (8, "T-HO GA"),
            (66, "1"),
            (2, "HG"),
            (10, "100"),
            (20, "200"),
            (0, "ATTRIB"),
            (8, "0"),
            (10, "100"),
            (20, "200"),
            (2, "TEN"),
            (1, "A1"),
            (0, "ATTRIB"),
            (8, "0"),
            (10, "100"),
            (20, "201"),
            (2, "TK"),
            (1, "3,43"),
            (0, "ATTRIB"),
            (8, "0"),
            (10, "100"),
            (20, "202"),
            (2, "layer"),
            (1, "x"),
            (0, "SEQEND"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let l = read("a.dxf", &d, &Options::default()).unwrap();
        let pts = &l[0].0;
        assert_eq!(pts.features.len(), 2, "the block's point and the insert's point; no ATTRIB points");
        let col = |name: &str| pts.field_index(name).unwrap();
        let row = (0..pts.features.len()).find(|&r| pts.value(r, col("entity")) == &Value::Text("INSERT".into())).unwrap();
        assert_eq!(pts.value(row, col("TEN")), &Value::Text("A1".into()));
        assert_eq!(pts.value(row, col("TK")), &Value::Text("3,43".into()));
        assert_eq!(pts.value(row, col("layer_attr")), &Value::Text("x".into()));
        assert_eq!(pts.value(row, col("layer")), &Value::Text("T-HO GA".into()));
        assert_eq!(pts.value(row, col("block")), &Value::Text("HG".into()));
        assert_eq!(pts.features[row].geometry, Some(Geometry::Point(Coord::new(100.0, 200.0))));

        let old = read("a.dxf", &d, &Options { attribs_as_points: true, ..Options::default() }).unwrap();
        assert_eq!(old[0].0.features.len(), 4, "the block's point and three ATTRIB points");
        assert!(old[0].0.field_index("TEN").is_none());
    }

    /// At most `MAX_TAG_FIELDS` tags become fields; the values under the others are counted.
    #[test]
    fn attribute_tags_are_bounded() {
        let mut p: Vec<(i32, String)> = vec![
            (0, "SECTION".into()),
            (2, "ENTITIES".into()),
            (0, "INSERT".into()),
            (66, "1".into()),
            (2, "NONE".into()),
            (10, "0".into()),
            (20, "0".into()),
        ];
        for i in 0..100 {
            p.extend([(0, "ATTRIB".into()), (10, "0".into()), (20, "0".into()), (2, format!("T{i}")), (1, "v".into())]);
        }
        p.extend([(0, "SEQEND".into()), (0, "ENDSEC".into()), (0, "EOF".into())]);
        let text: String = p.iter().map(|(c, v)| format!("{c:>3}\n{v}\n")).collect();
        let l = read("t.dxf", text.as_bytes(), &Options::default()).unwrap();
        assert_eq!(l[0].0.fields.len(), fields().len() + MAX_TAG_FIELDS);
        assert!(l[0].1.messages.iter().any(|m| m.contains("36 values of block attributes")), "{:?}", l[0].1.messages);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p rustgis-data --lib dxf::tests::block_attributes`
Expected: compile error (`attribs_as_points`, `MAX_TAG_FIELDS` don't exist).

- [ ] **Step 3: Implement**

Constants, beside the others near the top:

```rust
/// The most attributes one INSERT keeps.
const MAX_ATTRIBS: usize = 256;
/// The most attribute tags that become fields in one drawing (the rest are counted).
const MAX_TAG_FIELDS: usize = 64;
```

`Options` gets a field (it stays `Copy` and `Default`; the default is the new behaviour):

```rust
    /// Each ATTRIB as a point of its own with its text, as before attributes became fields.
    pub attribs_as_points: bool,
```

and its doc comment line becomes `/// What to read: the coordinate system of a drawing (which never says), one DXF layer only,
/// whether the layouts' entities (paper space) are drawn with the model's, and how block attributes come.`

`Insert` gets, after `fallback_text`:

```rust
    /// The attributes that follow it (tag, text), when they become fields.
    attribs: Vec<(String, String)>,
```

and both constructors in `entity()` (`"INSERT" => Shape::Insert(Insert { … })` and the DIMENSION one) add `attribs: Vec::new(),`.

`drawing()` takes the flag: `fn drawing(records: &[Record], attribs_as_points: bool) -> Drawing`. In `read()` the call is `drawing(&recs, opts.attribs_as_points)`; in `layers()` it is `drawing(&recs, true)` (ATTRIB layers stay listed). In `drawing()`, right after the `if matches!(r.kind, "SEQEND" | "VERTEX") || …` skip:

```rust
                // A block's attributes follow its INSERT (up to its SEQEND): kept with it, as fields.
                if r.kind == "ATTRIB" && !attribs_as_points {
                    let last = match &mut block {
                        Some((_, _, ents, _)) if section == "BLOCKS" => ents.last_mut(),
                        _ if section == "ENTITIES" => d.entities.last_mut(),
                        _ => None,
                    };
                    if let Some(Ent { kind: "INSERT", shape: Shape::Insert(ins), .. }) = last
                        && let Some(tag) = g.s(2).map(str::trim).filter(|t| !t.is_empty())
                    {
                        if ins.attribs.len() < MAX_ATTRIBS {
                            ins.attribs.push((tag.to_string(), plain_text(g.s(1).unwrap_or(""))));
                        }
                        continue;
                    }
                }
```

(An ATTRIB not following an INSERT is drawn as a point, as before.)

`Out` gets:

```rust
    /// Attribute tags made fields of the points layer so far ([`MAX_TAG_FIELDS`]), and values
    /// left out under tags beyond them.
    tag_fields: usize,
    tags_dropped: usize,
```

and the literal in `read()` gets `tag_fields: 0, tags_dropped: 0,`.

A new function after `emit`:

```rust
/// An INSERT's attributes: a point at its insertion point, a field per tag (a tag named like a
/// built-in field gets `_attr`).
#[allow(clippy::too_many_arguments)]
fn attribute_point(e: &Ent, ins: &Insert, xf: &Xf, layer: &str, color: i64, linetype: Option<&str>, paper: bool, out: &mut Out) {
    if out.vertices >= MAX_TOTAL_VERTICES {
        out.vertex_capped = true;
        return;
    }
    let at = xf.apply(Xf::ocs(ins.extrusion).apply(ins.at));
    if !finite(&[at]) {
        out.cut += 1;
        return;
    }
    out.vertices += 1;
    let attrs = vec![
        Value::Text(layer.to_string()),
        Value::Text(e.kind.to_string()),
        Value::Int(color),
        linetype.map(|l| Value::Text(l.to_string())).unwrap_or_default(),
        Value::Null,
        Value::Null,
        Value::Null,
        Value::Text(ins.block.clone()),
        Value::Float(at[2]),
        Value::Bool(paper),
    ];
    out.points.push_next(Some(Geometry::Point(xy(at))), attrs);
    out.count += 1;
    let built_in = fields();
    for (tag, text) in &ins.attribs {
        let name = if built_in.iter().any(|f| f.name.eq_ignore_ascii_case(tag)) { format!("{tag}_attr") } else { tag.clone() };
        let i = match out.points.field_index(&name) {
            Some(i) => i,
            None if out.tag_fields >= MAX_TAG_FIELDS => {
                out.tags_dropped += 1;
                continue;
            }
            None => {
                out.tag_fields += 1;
                out.points.ensure_field(&name, FieldType::Text)
            }
        };
        if let Some(v) = out.points.features.last_mut().and_then(|f| f.attrs.get_mut(i)) {
            *v = Value::Text(text.clone());
        }
    }
}
```

In `emit()`, inside `if let Shape::Insert(ins) = &e.shape {`, as its first statement (before the cycle check, so a missing or external block keeps its attributes):

```rust
            if !ins.attribs.is_empty() && opts.layer.is_none_or(|w| layer.eq_ignore_ascii_case(w)) {
                attribute_point(e, ins, xf, layer, color, linetype(), e.paper || from.paper, out);
            }
```

In `read()`, after the external-references message:

```rust
    if out.tags_dropped > 0 {
        report.messages.push(format!(
            "{} values of block attributes were left out: at most {MAX_TAG_FIELDS} attribute tags become fields",
            out.tags_dropped
        ));
    }
```

Module doc, lines 5–6: replace `LEADER, TEXT, MTEXT and\n//! ATTRIB (points with their text)` with `LEADER, TEXT and MTEXT\n//! (points with their text), block attributes (fields of a point where the block is inserted)`, and add to the second paragraph: `External references (XREF) aren't read; the report names their files.`

Engine, `crates/engine/src/cmd/layer.rs:909-913`, the literal becomes:

```rust
            let opts = rustgis_data::dxf::Options {
                crs: crs.map(rustgis_data::raster::Crs::parse).transpose()?,
                layer: ps(p, "layer").map(str::trim).filter(|l| !l.is_empty()),
                paper_space: b(p, "paperSpace").unwrap_or(false),
                attribs_as_points: false,
            };
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rustgis-data --lib dxf::`
Expected: all PASS, including `block_case_paper_space_and_attributes` (its ATTRIB follows a LINE, so it stays a point).

Run: `cargo build -p rustgis-engine`
Expected: builds.

- [ ] **Step 5: Commit**

```bash
git add crates/data/src/dxf.rs crates/engine/src/cmd/layer.rs
git commit -m "feat(dxf): block attributes become fields of a point at the insert

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: User guide

**Files:**
- Modify: `docs/user-guide/data-formats.md:31`

- [ ] **Step 1: Edit the DXF row**

Replace `TEXT, MTEXT, ATTRIB and constant ATTDEF (points with their text, at the alignment point of justified text)` with `TEXT, MTEXT and constant ATTDEF (points with their text, at the alignment point of justified text)`.

Replace `Not read: binary DXF,` with `A block's attributes (ATTRIB) become text fields of a point where the block is inserted, one per tag (up to 64 tags; a tag named like a built-in field gets \`_attr\`). External references (XREF) aren't read: a notice names their files (bind them in CAD to include them). Not read: binary DXF,`.

- [ ] **Step 2: Commit**

```bash
git add docs/user-guide/data-formats.md
git commit -m "docs: DXF block attributes and external references

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: CAD layer name patterns

**Files:**
- Create: `crates/analysis/src/cad.rs`
- Modify: `crates/analysis/src/lib.rs:13` (add `pub mod cad;` before `pub mod convert;`)

- [ ] **Step 1: Write the module with the failing test**

```rust
//! Digitising CAD drawings: tools over the layers a DXF or DWG drawing is read into
//! (`rustgis_data::dxf`), which keep each entity's CAD layer in the `layer` field.

/// Whether a CAD layer name matches a pattern: ASCII case-insensitive, `*` any run of characters.
pub fn matches(pattern: &str, name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

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
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p rustgis-analysis --lib cad::tests::patterns`
Expected: FAIL.

- [ ] **Step 3: Implement** (replace the body of `matches`)

```rust
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
```

- [ ] **Step 4: Run it**

Run: `cargo test -p rustgis-analysis --lib cad::tests::patterns`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/analysis/src/cad.rs crates/analysis/src/lib.rs
git commit -m "feat(analysis): CAD layer name patterns

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The `cadLayers` tool

**Files:**
- Modify: `crates/analysis/src/cad.rs`
- Modify: `crates/analysis/src/tools.rs:538` (new `ToolSpec` before the `];` closing `TOOLS`)

- [ ] **Step 1: Write the failing tests** (add to `mod tests` in `cad.rs`; the `use` lines go at the top of `mod tests`)

```rust
    use crate::{Inputs, Output, OutputLayer, ToolResult};
    use rustgis_data::{Coord, Dataset, Field, FieldType, Geometry, GeometryType, Value};
    use serde_json::json;

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
        let outs = vectors(split_layers(&drawing(), &json!({"input": "pts"})).unwrap());
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
        let outs = vectors(split_layers(&drawing(), &json!({"input": "pts", "layers": "ONG *"})).unwrap());
        assert_eq!(outs.len(), 2);
        let outs = vectors(split_layers(&drawing(), &json!({"input": "pts", "layers": "DAT_*", "group": "Prefix"})).unwrap());
        assert_eq!(outs.len(), 1);
        assert_eq!((outs[0].name.as_str(), outs[0].features.len()), ("DAT", 2));
        let outs = vectors(split_layers(&drawing(), &json!({"input": "pts", "decimalComma": false})).unwrap());
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].fields[tk].ty, FieldType::Text);
        assert!(split_layers(&drawing(), &json!({"input": "pts", "layers": "NOPE"})).is_err());
    }

    #[test]
    fn mixed_columns_stay_text_and_layer_field_is_required() {
        let mut d = drawing();
        d.0.features[1].attrs[1] = Value::Text("A1".into());
        d.0.features[1].attrs[0] = Value::Text("ONG 300".into());
        let outs = vectors(split_layers(&d, &json!({"input": "pts"})).unwrap());
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].fields[tk].ty, FieldType::Text, "one value that isn't a number keeps the column text");
        let plain = One(Dataset::new("pts", GeometryType::Point, vec![Field::new("v", FieldType::Text)]));
        assert!(split_layers(&plain, &json!({"input": "pts"})).unwrap_err().contains("'layer'"));
    }

    #[test]
    fn outputs_are_bounded() {
        let mut d = Dataset::new("pts", GeometryType::Point, vec![Field::new("layer", FieldType::Text)]);
        for i in 0..MAX_OUTPUTS + 100 {
            d.push(Some(Geometry::Point(Coord::new(0.0, 0.0))), vec![Value::Text(format!("L{i}"))]);
        }
        let r = split_layers(&One(d), &json!({"input": "pts"})).unwrap();
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
        let outs = vectors(split_layers(&One(pts), &json!({"input": "pts"})).unwrap());
        assert_eq!(outs.len(), 1);
        let tk = outs[0].field_index("TK").unwrap();
        assert_eq!(outs[0].value(0, tk), &Value::Float(3.43));
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p rustgis-analysis --lib cad::`
Expected: compile error (`split_layers`, `MAX_OUTPUTS` don't exist).

- [ ] **Step 3: Implement** (in `cad.rs`, above `#[cfg(test)]`; the `use` lines go under the module doc)

```rust
use std::collections::HashMap;

use rustgis_data::{Dataset, FieldType, Value};
use serde_json::Value as Json;

use crate::{Inputs, Output, OutputLayer, ToolResult};

/// The most layers one run makes (a hostile drawing can have thousands of CAD layers).
pub const MAX_OUTPUTS: usize = 500;

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
    let rows: Vec<usize> =
        if selected.is_empty() { (0..ds.features.len()).collect() } else { selected.iter().copied().filter(|r| *r < ds.features.len()).collect() };

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
        let is_number = field.ty == FieldType::Text
            && rows.iter().map(|&r| ds.value(r, f)).filter(|v| !blank(v)).all(|v| number(v, comma).is_some());
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
```

Register the tool: in `crates/analysis/src/tools.rs`, insert before the `];` at line 538:

```rust
    ToolSpec {
        id: "cadLayers",
        name: "Split CAD Layers",
        toolbox: "Conversion › CAD",
        summary: "Splits a layer read from a DXF or DWG drawing into one layer per CAD layer, or per name prefix.",
        params: &[
            P { name: "input", label: "Input Features", kind: K::VectorLayer, required: true, default: "", help: "A layer read from a DXF or DWG drawing (it has a 'layer' field)." },
            P { name: "layers", label: "CAD Layers", kind: K::Text, required: false, default: "", help: "The CAD layers to keep, separated by ';', with * for any characters (DAT_*;ONG *). Empty keeps every layer." },
            P { name: "group", label: "One Layer Per", kind: K::Choice(&["Each layer", "Prefix"]), required: false, default: "\"Each layer\"", help: "Prefix: the part of a name before its first '_' (DAT_CT_Dichvu goes to DAT)." },
            P { name: "decimalComma", label: "Read Decimal Commas", kind: K::Bool, required: false, default: "true", help: "A text field whose every value is a number, written with a decimal comma (4,17) or point, becomes a number field." },
        ],
        run: crate::cad::split_layers,
    },
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rustgis-analysis --lib cad::`
Expected: all PASS.

Run: `cargo test -p rustgis-analysis --lib tools::tests::hostile_params_never_panic tools::tests::no_built_in_tool_writes_its_output_as_a_file`
Expected: PASS (`cadLayers` runs on junk without panicking; on the test layers it errors on the missing `layer` field, which is allowed).

- [ ] **Step 5: Commit**

```bash
git add crates/analysis/src/cad.rs crates/analysis/src/tools.rs
git commit -m "feat(analysis): Split CAD Layers tool

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Gates and a check on the sample drawings

- [ ] **Step 1: Full CI**

Run: `cargo xtask ci`
Expected: fmt, clippy `-D warnings`, tests and layers pass. Fix and amend the task's commit with a new commit if clippy flags something (`too_many_arguments` is allowed on `attribute_point` already).

- [ ] **Step 2: wasm**

Run: `cargo build --target wasm32-unknown-unknown -p rustgis-web`
Expected: builds. (Needs clang for the Zstandard decoder; if clang isn't installed, say so in the hand-off instead of skipping silently.)

- [ ] **Step 3: Sample drawings, by hand**

With the ODA File Converter installed (`C:\Program Files\ODA\ODAFileConverter 27.9.0`):

```bash
cargo run --release -p rustgis-cli -- run --cmd 'layer.addData={"path":"01.CAD.KCN.VB1/2.HTKT/Capthoatnuoc/QH12TNT.dwg"}' --cmd 'gp.run={"tool":"cadLayers","params":{"input":"QH12TNT points","layers":"0;T-HO GA"}}' --cmd 'share.exportMap={"path":"qh12.png"}'
```

Expected: the read notice names 4 external drawings (`XR. MC.dwg`, `Xr.kientruc.KCN.VB1.dwg`, `Xr.Khung A0 Fix.KCN.VB1.1.2000.CTN.dwg`, `Xr.nen.HIENTRANG.dwg`); the points layer has `TEN`, `TK`, `TN`, `QH`, `X`, `Y` fields; after the tool, `TK` and `TN` are number fields. If the points layer is named differently, list layers with `--cmd 'info.layers={}'` first. Don't commit the sample files or the PNG.
