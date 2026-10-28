# Digitising planning drawings from CAD (DWG/DXF → layers → Shapefile)

## Intent
The user, a Vietnamese planner, digitises planning drawings (Circular 12/2016/TT-BXD) handed over
as AutoCAD files. Opening a DWG already works (ODA File Converter → DXF → `dxf::read`). What costs
the time is everything after:

1. **Layer separation**: one drawing holds 12–39 CAD layers mixed into three layers (points,
   lines, polygons); the user splits them by hand.
2. **Joining point data to lines**: manholes carry their data (name, levels) as block attributes;
   the user copies it onto the pipe runs by hand.
3. **Drawn vs annotated length**: a run is drawn through several manholes as one polyline, its
   dimension measures one stretch of it ("labelled 70 m, drawn 90 m"); the user cuts runs by hand.
4. **Broken linework**: ends that stop short, overshoot or cross without a node.

Like the Circular 12 work (`2026-10-26-tt12-land-use-design.md`), this lives in the user's private
fork; nothing goes upstream. AGENTS.md's rules hold (never crash, never break wasm, translatable
text, `cargo xtask ci`).

## What the sample drawings show
`01.CAD.KCN.VB1` (the user's project, not committed): 38 DWG (AutoCAD 2018), 20 of them external
references. Converted and inspected: QH04 (land use), QH06 (roads), QH07, QH08 (water supply), QH09,
QH12 (wastewater).

- **Codes are in CAD layer names**: pipe diameter in `ONG 300`, `ONG 500`, `ong tnm B1000`; land use
  in `DAT_CTSX_Congnghiep`, `DAT_CT_Dichvu`, `DAT_HTKT_Baidoxe`.
- **Point data is in block attributes (ATTRIB)**: a manhole block has `TEN` (A1, B4…), `TK` (design
  level), `TN` (ground level), `X`, `Y`; a plot table block (`Chitieu`) has `A` (plot code CN.01),
  `B` (density %), `C` (storeys), `D` (area ha), `E` (land-use coefficient). `dxf::read` today
  emits every ATTRIB as its own point, so 5 attributes become 5 unrelated points.
- **Lengths are DIMENSIONs**, not text: aligned dimensions (type 33) whose `measure` (code 42)
  equals the distance between their points 13 and 14. One real case in QH12: an `ONG 500` run
  drawn 195.87 m, its dimension 130.29 m with both ends on the run (0.1 m off). **The dimension's
  ends are where the cut belongs.**
- **Decimal comma**: `4,17` is 4.17.
- **VN-2000 coordinates**, with surveyors' X = northing, Y = easting in the attributes
  (`X=2308552.345`, `Y=397294.629` for drawing point (397294.6, 2308552.3)).
- **Linework** (pipe-run end to nearest other run): QH12 26/52 ends joined exactly, 2 within
  0.01–0.5 m, 2 within 0.5–3 m, 2 within 3–10 m, 20 more than 10 m (network ends). QH08: 24
  joined, 2 within 0.5 m, 8 ends; 2 runs cross mid-span without a node.
- **Plots** (QH04): every `DAT_*` polyline is closed and has a HATCH on the same boundary; the
  65 `Chialo` (plot division) polylines are all open.
- **External references**: block records with flag 70 bit 4 and a relative path in code 1
  (`.\Xr.nen.HIENTRANG.dwg`). `dxf::read` counts their INSERTs as `cut` and says nothing.

## Principle: fix what is certain, ask about the rest
Every problem a tool finds has a confidence. Certain → the tool fixes it and records the fix.
Not certain → the tool proposes a fix and the user decides. Commands never open dialogs, so asking
is done with data:

1. The tool writes its result layer and a **proposals** point layer: `kind`, `note`,
   `confidence`, `decision` (empty), plus what the fix needs (`target` row, `at`, `to`).
2. The user reviews it in the attribute table or Error Inspector (each row zooms to its place),
   sets `decision` to `accept` or `skip`, or fixes the line by hand with `edit.*`.
3. An apply tool applies the accepted proposals in one undoable step; undecided ones stay and are
   counted.

The AI Assistant uses the same path: it walks the proposals, zooms to each, asks in chat, writes
`decision`. No assistant-specific code.

Two thresholds, in metres, user-adjustable: `snap` (default 0.5: below it a gap is a drawing slip,
fixed) and `ask` (default 10: between `snap` and `ask` it is proposed; beyond it it is a real end,
reported only when it has no node). Defaults come from the measurements above.

## Phase 1: `cadLayers`, attributes as fields, external references reported

### In `rustgis-data` (`dxf.rs`)
- **Attributes as fields.** The ATTRIBs following an INSERT become fields of the insert's point
  (field name = tag, value = text); the separate ATTRIB points are no longer made. Tags are
  case-folded to one field each; a tag that collides with a built-in field (`layer`, `text`…) gets
  the suffix `_attr`. At most 64 tag fields per drawing (bounded; the rest are counted in the
  report). `Options::attribs_as_points` keeps the old behaviour.
- **External references reported.** A block record with flag 70 bit 4 is an external reference:
  its INSERTs are counted per reference, and the read report says
  "3 external drawings not read: Xr.nen.HIENTRANG.dwg, …; bind them in CAD to include them".
  Not read here (later work: it needs `Services` on the web and an ODA run per reference).
- **Dimensions keep their measured points.** A linear DIMENSION also makes a line between its
  measured points 13 and 14, with its measure (42) in a `measure` field (phase 2).

### In `rustgis-analysis` (new `cad.rs`): tool `cadLayers` ("Split CAD Layers", Conversion › CAD)
| Parameter | Kind | Default | Meaning |
|---|---|---|---|
| `input` | VectorLayer, required | | a layer read from DXF/DWG |
| `layers` | Text | all | CAD layers to keep, `;`-separated, `*` wildcard (`DAT_*;ONG *`); ASCII case-insensitive |
| `group` | Choice `Each layer` / `Prefix` | `Each layer` | one output per CAD layer, or per prefix before the first `_` |
| `decimalComma` | Bool | true | a text field whose every non-empty value reads as a number with `,` decimals becomes a number field |

Output `Output::Layers`, one vector layer per group, named by the CAD layer (or prefix), keeping
only fields with a value in that group. Input without a `layer` field is an error naming the field.
At most 500 outputs (a hostile drawing can have thousands of layers); the rest are counted.

## Phase 2: `cadNetwork` and `cadNetworkApply`

### `cadNetwork` ("Build Network From CAD", Conversion › CAD)
| Parameter | Kind | Default |
|---|---|---|
| `lines` | VectorLayer, required | pipe/cable runs |
| `layers` | Text | all | CAD layers of `lines` to use, `;`-separated, `*` wildcard (`ONG *`) |
| `nodes` | PointLayer | manholes, poles |
| `nodeName` | Text | empty | field naming nodes; empty uses `TEN`, else `A` |
| `dimensions` | VectorLayer | dimensions from `cadLayers` (their measured points 13/14 and `measure`) |
| `proposals` | PointLayer | earlier run's proposals with `decision` set: accepted fixes are made |
| `snap`, `ask` | Number | 0.5, 10 |
| `lengthTolerance` | Number | 0.5 |

Steps:
1. **Clean linework.** Gaps and overshoots below `snap`: fixed (end moved to the run / node).
   Exact duplicates on the same CAD layer: one kept. Recorded as `fixed` proposals (`decision`
   already `accept`) so every change is visible.
2. **Cut at nodes.** A node within `snap` of a run cuts it there (no cut within `snap` of an end).
3. **Join node data.** Each piece gets `node1`, `node2` (names) and the end nodes' fields prefixed
   `n1_`, `n2_`.
4. **Match dimensions.** A dimension whose points 13 and 14 both lie within `snap` of a run belongs
   to the stretch between them: `len_noted` = its measure.
5. **Compare lengths.** `len_drawn` is the sum of `rustgis_geom::wgs84_distance_m` (the ellipsoid:
   the sphere is off by up to 0.5 %, 4 m on 806 m). Web-Mercator metres are not used: at latitude 21°
   they are 7 % long. The VN-2000 grid scale factor (0.9999 at the central meridian, up to about 1.0003
   at a 3° zone's edge) makes geodesic and drawing lengths differ by ≤0.04 % (0.3 m on 806 m), inside
   `lengthTolerance`.

Proposals (never applied without `accept`):
| `kind` | When |
|---|---|
| `gap` / `overshoot` | between `snap` and `ask` |
| `crossing` | runs cross mid-span with no node (may be pipes at different depths) |
| `node_off_line` | node between `snap` and `ask` from a run |
| `cut_at_dimension` | a dimension end falls mid-piece: cut there |
| `duplicate` | a line drawn twice across CAD layers (on one layer it is dropped, recorded as fixed) |

Partial overlaps aren't detected yet: added when a drawing has some.

Reports only (no fix to propose): `length_mismatch` (|drawn − noted| > `lengthTolerance` with no
cut that explains it), `piece_without_node`, `piece_without_dimension`, `stray_node`,
`stray_dimension` (a dimension on no line), `dangling_end` (end beyond `ask` from anything, with no node).

Outputs: the network (lines, input fields + `node1`, `node2`, `n1_*`, `n2_*`, `len_drawn`,
`len_noted`, `len_diff`) and the proposals.

### Applying decisions
Built as a re-run, not a separate tool: `cadNetwork` with `proposals` (an earlier run's
proposals layer, `decision` set) makes every accepted fix, leaves skipped ones (also certain
fixes the user skipped) and writes undecided ones again. A finding is matched to its decision by
kind and place (within `snap`, at least 5 cm). Hand fixes go on the input lines (`edit.*`), so
every re-run keeps them; undo is the run's `gp.run` step; each run adds a new network and
proposals layer. Why: `Output::Update` replaces one layer, and applying changes two (network and
proposals) and needs the nodes and dimensions again.

## Phase 3: `cadParcels`
### `cadParcels` ("Build Plots From CAD", Conversion › CAD)
| Parameter | Kind | Default |
|---|---|---|
| `polygons` | VectorLayer, required | closed plot polylines and hatches (`DAT_*`) |
| `splitLines` | VectorLayer | plot division lines (`Chialo`) |
| `labels` | PointLayer | plot tables (`Chitieu`) |
| `snap`, `ask` | Number | 0.5, 10 |

Steps:
1. Closed polylines and hatches → polygons; a hatch on the same boundary as a polyline (area
   within 0.1 %, same CAD layer) counts once.
2. Split polygons with `splitLines`: lines noded, ends within `snap` snapped, rings traced
   (polygonize, written here: no new dependency).
3. Each table inside a plot gives the plot its fields (`A`…`E` kept as named).
4. Circular 12 code from the CAD layer name and the plot code prefix (`CN.01` → `CN`) through a
   mapping table beside `tt12::GROUPS` in `crates/map/src/tt12.rs`. **The builder drafts the
   mapping; the user reviews it before this step is finished** (as with the group names). The
   `layer` field is set, so `landuse.tt12` styles the result.

Proposals: division-line `gap` between `snap` and `ask`; `two_tables` (one plot, two tables:
split the plot or drop a table); `table_outside` (table in no plot); `code_conflict` (layer name and
plot prefix give different codes); `unmapped` (no code: user picks one). Reports: `no_table`,
`area_mismatch` (geodesic area vs `D` ha beyond 1 %), `overlap` and `gap` between plots
(`topology::overlaps`, `topology::gaps`). Applied the same way: run again with the decided
proposals.

`rustgis-analysis` doesn't depend on `rustgis-map`; the mapping table goes to the tool as a
parameter filled by the engine, or moves to `analysis`. Decided in the phase 3 plan.

## Testing
- Each tool: small DXF built in the test (as `dxf.rs` tests do) covering a 70/90 run with a
  dimension, a 0.3 m gap (fixed), a 5 m gap (proposed), a crossing (proposed), a plot with two
  tables, decimal commas, ATTRIB fields, an external reference reported.
- Every tool joins `hostile_params_never_panic`; bounded counts everywhere.
- Checked by hand on the converted sample drawings: counts of fixes and proposals against the
  measurements above. The samples are the user's project files and are not committed.

## Non-goals
- Reading external references (reported only, see phase 1).
- A map tool for clicking proposals (the attribute table and Error Inspector do it).
- Shapefile export changes: `share.export`/`exportFeatures` already write Shapefiles; field-name
  length (10 chars) and encoding are checked when the export spec is written.
- Text annotations as length sources (`L=70m` as TEXT): the samples use dimensions only.
