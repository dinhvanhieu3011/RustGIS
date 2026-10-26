# Land-use planning maps to Circular 12 (Thông tư 12/BXD)

## Intent
The user, a Vietnamese planner, keeps a private fork of RustGIS for digitising land-use planning
maps ("quy hoạch sử dụng đất"). Each map has to follow the Ministry of Construction's Circular 12
(Thông tư 12/BXD): every land parcel carries a land-use code, and its colour and fill pattern
depend on that code. Today they would set this up by hand for every layer. They want one action
that makes a polygon layer "a Circular 12 land-use layer": its codes right, its style right, and
digitising it fills the code in by itself.

The fork is not upstream RustGIS: the user drew the pattern images themselves and ships them in
the fork. Nothing from this work goes to `opengeos/RustGIS`.

## Source of truth
- `QH_SDD_TT12BXD.xml` (repo root): a GeoServer SLD. Field `layer` holds the code. 36 land-use
  groups (ODV, ONN, OLK, OCC, OHH, OBT, OLX, CXGPC, CDO, TH, THCS, CXD, CXO, DCN, DKT, DGD, DCQ,
  DYT, DDL, DTG, DHK, XCD, DTT, CXCL, DAQ, DNT, DNN, DLN, MNC, VDT, VNT, VNN, RDD, VQG, DNL, DLM).
  Each group lists the codes in it (742 literals in all, e.g. ODV takes ODV, ODV01, ODV02, OM,
  OM01, …; CXGPC takes 92), and has two rules:
  - 1:10,000 to 1:1,000,000: solid fill in the group colour (e.g. ODV `#ff7c01`), no outline of
    its own.
  - 1:1,000 to 1:10,000: fill with the group's PNG from `img/`, tiled, outline in the group colour.
  - A first rule outlines every polygon in `#4D4D4D`.
- `img/` (repo root): 435 PNGs drawn by the user. Six that the SLD names are missing:
  `MatNuoc`, `RungDacDung`, `VungNongNghiep`, `VungPhatTrienDanCuNongThon`,
  `VungPhatTrienDothi`, `VuonQuocGia`. Those groups show their colour at every scale until the
  user adds the images. DLM reuses `DatCayXanhChuyenDe.png` (as the SLD says).
- Codes in two groups (user's decision: the later rule wins, as GeoServer draws it today):
  `CDO03` belongs to CXO, `BDX01` to MNC.
- Outline colours per group, as the SLD gives them: every group outlines in its fill colour at
  image scales except DNL, whose outline is `#FFBF7F` (user's decision: keep it).
- The SLD has codes only, no Vietnamese names. The name list ("Đất ở đô thị" → ODV …) is drafted
  by the builder from Circular 12, kept in Rust source (changing it means a rebuild), and
  **reviewed by the user before the name matching is finished**: the builder sends the draft
  list early, and the matching step waits for the reviewed list.

## What the user does
1. Opens or creates a polygon layer and runs one command (working name `landuse.tt12`) on it.
2. The command:
   - Adds a text field `layer` if the layer has none.
   - If the user names a source field holding land-use names in words (e.g. a `loaidat` column
     reading "Đất ở đô thị"), fills `layer` with the matching group code. Matching ignores case,
     Vietnamese diacritics and surrounding spaces. A row whose name matches nothing keeps `layer`
     empty and is counted in the result ("12 parcels: no Circular 12 name matched"), so the user
     can fix them in the attribute table. The source field is kept untouched.
   - If `layer` already holds codes, leaves them as they are.
   - Sets the layer's style to Circular 12 (below).
   - Adds one feature template per group (36), named by its Vietnamese name and code
     (e.g. "Đất ở đô thị (ODV)"), defaulting `layer` to the group code.
   - Is one undoable step.
3. Digitising: the user picks a template in the Editing pane and draws. The new parcel has its
   code and shows in the right style at once. Nothing new here: templates already do this
   (`edit.template`, `edit.addFeature` with `template`).

## How the layer is drawn
- A feature's group comes from its `layer` value: the group whose code list holds it, exactly as
  the SLD lists them. Values in no group draw in a neutral grey and appear in the legend as
  "<all other values>".
- Zoomed out further than 1:10,000: solid group colour, `#4D4D4D` outline.
- Zoomed in to 1:10,000 or closer: the group's image tiled over the parcel, outline in the group
  colour. Where the image is missing, the solid colour instead.
- Outside the SLD's scale range (closer than 1:1,000, further than 1:1,000,000) the SLD draws
  nothing. **Builder's call, confirmed in conversation as a guess:** keep drawing: images closer
  than 1:1,000, colour further than 1:1,000,000, so parcels never vanish.
- Legend: one row per group, its name and colour.
- At minimum the colours must be right everywhere the map is drawn (the map view, PNG export,
  layouts). Images are the goal; colour is the floor.

## Goals
- One command turns a layer into a Circular 12 layer; digitising then needs no typing of codes.
- Colours and groups exactly as the SLD.
- Image fills at large scales, from the user's images.

## Non-goals
- A general SLD importer. The Circular 12 table is built in; the SLD file is the reference the
  builder reads, not something the app parses at run time.
- Matching codes from other standards (e.g. the Ministry of Natural Resources' land codes).
- Upstreaming. CI gates that only matter to upstream (`ATTRIBUTION.md` rows for the images) may
  be skipped in the fork; everything else in AGENTS.md (never crash, never break wasm,
  translatable text, `cargo xtask ci`) still holds.

## Anti-goals
- A parcel showing the wrong colour for its code: worse than grey.
- Silently guessing a code for a name that doesn't match: unmatched stays empty and is reported.
- Overwriting codes the user already entered.

## Constraints
- Desktop first (user's decision). Images are read at run time from an `img/` folder (beside the
  executable, or a folder the user sets), so a new image shows without a rebuild. A missing or
  unreadable image falls back to the colour, never a crash.
- The web build must still compile (AGENTS.md); on the web, image fills fall back to colour.
- The map view has a CPU (tiny-skia) and a GPU path; both must show at least the colour.

## Left to the builder
- Where the group table lives (likely `rustgis-map`), and how image fills enter the symbology
  model (`Symbol` is `Copy`; a per-class picture beside it is one way).
- Image tile size on screen (likely the PNG's own pixel size at 96 dpi).
- Exactly where the `img/` folder is looked for, and how the user points to another one.
- Whether the GPU path draws images or falls back to colour first.
- Ribbon placement of the command, following the ribbon guidelines.
- The Vietnamese name list and its aliases (draft, then the user reviews it).
