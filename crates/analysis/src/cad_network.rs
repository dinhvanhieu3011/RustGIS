//! `cadNetwork`: a pipe or cable network from a CAD drawing's lines (phase 2 of
//! `docs/superpowers/specs/2026-10-28-cad-digitising-design.md`). It works in four steps:
//! 1. Ends that stop short of a line or node, or run past a line, are joined.
//! 2. Lines are cut at the nodes (manholes) on them and take the nodes' data.
//! 3. Dimensions give the length noted on the drawing, which is compared with the length drawn.
//! 4. What is certain is fixed and recorded. What isn't becomes a proposal the user accepts or
//!    skips; running again with those proposals makes the accepted fixes.
//!
//! Distances are worked out in metres on a plane centred on the lines (azimuthal equidistant
//! on WGS 84). Lengths are measured on the ellipsoid.

use rustgis_data::{Dataset, Field, FieldType, Geometry, GeometryType, Value};
use rustgis_geom::{Aeqd, Coord, Extent, wgs84_distance_m};
use serde_json::Value as Json;

use crate::{Inputs, Output, OutputLayer, ToolResult};

/// Most line parts, nodes and dimensions read (a hostile drawing can have millions).
const MAX_ITEMS: usize = 50_000;
/// Most findings written; the rest are counted.
const MAX_FINDINGS: usize = 10_000;
/// Most tests (segment pairs, vertices visited) one run spends, so a huge layer can't stall
/// the app.
const MAX_WORK: usize = 50_000_000;
/// Closer than this (metres), two points are the same.
const SAME: f64 = 1e-3;

fn dist(a: Coord, b: Coord) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn length(pts: &[Coord]) -> f64 {
    pts.windows(2)
        .map(|w| match w {
            [a, b] => dist(*a, *b),
            _ => 0.0,
        })
        .sum()
}

/// The point `s` metres along a line (its end beyond it).
fn point_at(pts: &[Coord], s: f64) -> Coord {
    let mut left = s.max(0.0);
    for w in pts.windows(2) {
        let [a, b] = w else { continue };
        let l = dist(*a, *b);
        if left <= l && l > 0.0 {
            let t = left / l;
            return Coord::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
        }
        left -= l;
    }
    pts.last().copied().unwrap_or_default()
}

/// The point of a line nearest `p`: how far it is, how far along the line, and where.
fn foot(pts: &[Coord], p: Coord) -> Option<(f64, f64, Coord)> {
    let mut best: Option<(f64, f64, Coord)> = None;
    let mut s0 = 0.0;
    for w in pts.windows(2) {
        let [a, b] = w else { continue };
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l = dx.hypot(dy);
        let t = if l > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / (l * l)).clamp(0.0, 1.0) } else { 0.0 };
        let q = Coord::new(a.x + t * dx, a.y + t * dy);
        let d = dist(p, q);
        if best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, s0 + t * l, q));
        }
        s0 += l;
    }
    best
}

/// Where segments ab and cd cross: the fractions along each.
fn cross(a: Coord, b: Coord, c: Coord, d: Coord) -> Option<(f64, f64)> {
    let (r, s) = ((b.x - a.x, b.y - a.y), (d.x - c.x, d.y - c.y));
    let den = r.0 * s.1 - r.1 * s.0;
    if den.abs() < 1e-12 {
        return None;
    }
    let t = ((c.x - a.x) * s.1 - (c.y - a.y) * s.0) / den;
    let u = ((c.x - a.x) * r.1 - (c.y - a.y) * r.0) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Where two lines cross: how far along each, and where. Each pair of segments tested adds
/// one to `work`; past [`MAX_WORK`] the rest aren't tested.
fn crossings(p: &[Coord], q: &[Coord], work: &mut usize) -> Vec<(f64, f64, Coord)> {
    let mut out = Vec::new();
    let mut s0 = 0.0;
    for w in p.windows(2) {
        let [a, b] = w else { continue };
        let la = dist(*a, *b);
        let mut t0 = 0.0;
        for v in q.windows(2) {
            let [c, d] = v else { continue };
            let lc = dist(*c, *d);
            *work += 1;
            if *work > MAX_WORK {
                return out;
            }
            if let Some((t, u)) = cross(*a, *b, *c, *d) {
                out.push((s0 + t * la, t0 + u * lc, Coord::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y))));
            }
            t0 += lc;
        }
        s0 += la;
    }
    out
}

/// A line cut at the distances along it `cuts` (sorted): its pieces, each with how far along
/// the line it starts.
fn split(pts: &[Coord], cuts: &[f64]) -> Vec<(f64, Vec<Coord>)> {
    let mut pieces = Vec::new();
    let mut cur: Vec<Coord> = pts.first().copied().into_iter().collect();
    let (mut start, mut s0, mut k) = (0.0, 0.0, 0);
    for w in pts.windows(2) {
        let [a, b] = w else { continue };
        let l = dist(*a, *b);
        while let Some(&c) = cuts.get(k) {
            if c > s0 + l {
                break;
            }
            let t = if l > 0.0 { ((c - s0) / l).clamp(0.0, 1.0) } else { 0.0 };
            let q = Coord::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
            cur.push(q);
            pieces.push((start, std::mem::replace(&mut cur, vec![q])));
            start = c;
            k += 1;
        }
        cur.push(*b);
        s0 += l;
    }
    pieces.push((start, cur));
    for (_, p) in &mut pieces {
        p.dedup();
    }
    pieces.retain(|(_, p)| p.len() >= 2);
    pieces
}

/// Whether two lines run through the same points (either way round).
fn same_line(a: &[Coord], b: &[Coord]) -> bool {
    a.len() == b.len()
        && (a.iter().zip(b).all(|(p, q)| dist(*p, *q) < SAME) || a.iter().zip(b.iter().rev()).all(|(p, q)| dist(*p, *q) < SAME))
}

/// The drawing's own fields (`rustgis_data::dxf`): not copied from nodes onto the lines.
const CAD_FIELDS: [&str; 10] = ["layer", "entity", "color", "linetype", "text", "height", "rotation", "block", "elevation", "paperspace"];

fn text_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Text(t) => t.clone(),
        v => v.to_string(),
    }
}

fn text<'a>(p: &'a Json, k: &str) -> &'a str {
    p.get(k).and_then(Json::as_str).unwrap_or("").trim()
}

/// An optional layer parameter.
fn layer<'a>(inp: &'a dyn Inputs, p: &Json, k: &str) -> Result<Option<&'a Dataset>, String> {
    match text(p, k) {
        "" => Ok(None),
        n => inp.dataset(n).map(Some).ok_or_else(|| format!("no layer named '{n}'")),
    }
}

fn lonlat(e: &Extent) -> bool {
    e.is_empty() || (e.min_x >= -180.0 && e.max_x <= 180.0 && e.min_y >= -90.0 && e.max_y <= 90.0)
}

fn is_dimension(d: &Dataset, entity: Option<usize>, row: usize) -> bool {
    entity.is_some_and(|i| matches!(d.value(row, i), Value::Text(t) if t.eq_ignore_ascii_case("DIMENSION")))
}

/// One part of an input line, on the plane.
struct Run {
    row: usize,
    layer: String,
    pts: Vec<Coord>,
    /// Where it is cut, as distances along it.
    cuts: Vec<f64>,
    /// Dropped (a duplicate).
    gone: bool,
}

struct Node {
    at: Coord,
    row: usize,
    name: String,
}

struct Dim {
    a: Coord,
    b: Coord,
    measure: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Level {
    /// Certain: fixed unless an earlier run's proposal skipped it.
    Fixed,
    /// Proposed: fixed when accepted.
    Ask,
    /// Reported: nothing to fix.
    Report,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Fixed => "fixed",
            Level::Ask => "ask",
            Level::Report => "report",
        }
    }
}

struct Finding {
    kind: &'static str,
    at: Coord,
    /// Where the fix moves `at` to, for a gap or an overshoot.
    to: Option<Coord>,
    /// The input line's row.
    line: Option<usize>,
    note: String,
    level: Level,
    decision: &'static str,
}

/// An earlier run's decided proposals: kind, where, `accept` or `skip`.
#[derive(Default)]
struct Decisions(Vec<(String, Coord, &'static str)>);

impl Decisions {
    fn read(d: &Dataset, plane: &Aeqd) -> Result<Decisions, String> {
        let (Some(k), Some(dc)) = (d.field_index("kind"), d.field_index("decision")) else {
            return Err(format!("'{}' isn't a proposals layer: it has no 'kind' and 'decision' fields", d.name));
        };
        let mut v = Vec::new();
        for (r, f) in d.features.iter().enumerate().take(MAX_FINDINGS) {
            let Some(Geometry::Point(c)) = &f.geometry else { continue };
            let decision = match text_of(d.value(r, dc)).trim().to_ascii_lowercase().as_str() {
                "accept" => "accept",
                "skip" => "skip",
                _ => continue,
            };
            v.push((text_of(d.value(r, k)), plane.forward(*c), decision));
        }
        Ok(Decisions(v))
    }

    fn get(&self, kind: &str, at: Coord, tol: f64) -> Option<&'static str> {
        self.0
            .iter()
            .filter(|(k, p, _)| k.eq_ignore_ascii_case(kind) && dist(*p, at) <= tol)
            .min_by(|a, b| dist(a.1, at).total_cmp(&dist(b.1, at)))
            .map(|(_, _, d)| *d)
    }
}

/// What a run found.
struct Book {
    found: Vec<Finding>,
    prior: Decisions,
    /// How near an earlier proposal, or one already found, counts as the same place.
    tol: f64,
    dropped: usize,
}

impl Book {
    /// Records a finding and says whether to make its fix: a certain one unless an earlier
    /// run's proposal skipped it, a proposed one when accepted. A finding already recorded (a
    /// gap seen from its other end) is recorded once.
    fn note(&mut self, kind: &'static str, at: Coord, to: Option<Coord>, line: Option<usize>, note: String, level: Level) -> bool {
        let tol = self.tol;
        let same = |f: &&Finding| {
            f.kind == kind && (dist(f.at, at) <= tol || f.to.zip(to).is_some_and(|(ft, t)| dist(ft, at) <= tol && dist(f.at, t) <= tol))
        };
        if let Some(f) = self.found.iter().find(same) {
            return f.decision == "accept";
        }
        let decision = match (level, self.prior.get(kind, at, tol)) {
            (Level::Report, _) => "",
            (_, Some(d)) => d,
            (Level::Fixed, None) => "accept",
            (Level::Ask, None) => "",
        };
        if self.found.len() < MAX_FINDINGS {
            self.found.push(Finding { kind, at, to, line, note, level, decision });
        } else {
            self.dropped += 1;
        }
        decision == "accept"
    }
}

/// The network being built, on the plane.
struct Net {
    runs: Vec<Run>,
    nodes: Vec<Node>,
    dims: Vec<Dim>,
    snap: f64,
    ask: f64,
    book: Book,
    work: usize,
    /// Where overshoots were found (fixed or not): crossings there aren't mid-line.
    overshoots: Vec<Coord>,
    /// Where nodes sit: each node's position, and the foot of a node off a line whose cut was
    /// accepted; with the node's index.
    anchors: Vec<(Coord, usize)>,
    /// Dimensions on lines: dimension index, run index, from and to along the run.
    on: Vec<(usize, usize, f64, f64)>,
}

fn read_runs(inp: &dyn Inputs, name: &str, lines: &Dataset, patterns: &[&str], plane: &Aeqd, capped: &mut bool) -> Vec<Run> {
    let (li, ei) = (lines.field_index("layer"), lines.field_index("entity"));
    let selected = inp.selection(name);
    let rows: Vec<usize> = if selected.is_empty() {
        (0..lines.features.len()).collect()
    } else {
        selected.iter().copied().filter(|r| *r < lines.features.len()).collect()
    };
    let mut runs = Vec::new();
    for r in rows {
        // A dimension's lines are what it measures, not a pipe.
        if is_dimension(lines, ei, r) {
            continue;
        }
        let layer = li.map(|i| text_of(lines.value(r, i))).unwrap_or_default();
        if !patterns.is_empty() && !patterns.iter().any(|p| crate::cad::matches(p, &layer)) {
            continue;
        }
        let Some(Geometry::Line(parts)) = lines.features.get(r).and_then(|f| f.geometry.as_ref()) else { continue };
        for part in parts {
            if runs.len() >= MAX_ITEMS {
                *capped = true;
                return runs;
            }
            let pts: Vec<Coord> = part.iter().map(|c| plane.forward(*c)).collect();
            if length(&pts) > SAME {
                runs.push(Run { row: r, layer: layer.clone(), pts, cuts: Vec::new(), gone: false });
            }
        }
    }
    runs
}

/// The nodes, named by `name_field` (else `TEN`, else `A`), and the fields copied from them.
fn read_nodes(ds: Option<&Dataset>, name_field: &str, plane: &Aeqd, capped: &mut bool) -> Result<(Vec<Node>, Vec<usize>), String> {
    let Some(ds) = ds else { return Ok((Vec::new(), Vec::new())) };
    let ni = match name_field {
        "" => ds.field_index("TEN").or_else(|| ds.field_index("A")),
        f => Some(ds.field_index(f).ok_or_else(|| format!("'{}' has no field '{f}'", ds.name))?),
    };
    let mut nodes = Vec::new();
    for (r, f) in ds.features.iter().enumerate() {
        let Some(Geometry::Point(c)) = &f.geometry else { continue };
        if nodes.len() >= MAX_ITEMS {
            *capped = true;
            break;
        }
        nodes.push(Node { at: plane.forward(*c), row: r, name: ni.map(|i| text_of(ds.value(r, i))).unwrap_or_default() });
    }
    let copied = (0..ds.fields.len())
        .filter(|i| ds.fields.get(*i).is_some_and(|f| !CAD_FIELDS.iter().any(|c| c.eq_ignore_ascii_case(&f.name))))
        .collect();
    Ok((nodes, copied))
}

/// The dimensions: lines with a `measure` (and, when there's an `entity` field, DIMENSION).
fn read_dims(ds: Option<&Dataset>, plane: &Aeqd, capped: &mut bool) -> Result<Vec<Dim>, String> {
    let Some(ds) = ds else { return Ok(Vec::new()) };
    let mi = ds.field_index("measure").ok_or_else(|| {
        format!("'{}' has no 'measure' field: give the lines layer of a drawing with dimensions (add the drawing again if it was added before dimensions were read)", ds.name)
    })?;
    let ei = ds.field_index("entity");
    let mut dims = Vec::new();
    for (r, f) in ds.features.iter().enumerate() {
        if ei.is_some() && !is_dimension(ds, ei, r) {
            continue;
        }
        let Some(Geometry::Line(parts)) = &f.geometry else { continue };
        let (Some(a), Some(b)) = (parts.first().and_then(|p| p.first()), parts.first().and_then(|p| p.last())) else { continue };
        let Some(measure) = ds.value(r, mi).as_f64().filter(|m| *m > 0.0) else { continue };
        if dims.len() >= MAX_ITEMS {
            *capped = true;
            break;
        }
        dims.push(Dim { a: plane.forward(*a), b: plane.forward(*b), measure });
    }
    Ok(dims)
}

/// `cadNetwork`: see the module.
pub fn network(inp: &dyn Inputs, p: &Json) -> Result<ToolResult, String> {
    let number = |k: &str, d: f64| {
        p.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())).filter(|v: &f64| v.is_finite()).unwrap_or(d)
    };
    let snap = number("snap", 0.5).clamp(0.0, 100.0);
    let ask = number("ask", 10.0).clamp(snap, 1000.0);
    let tolerance = number("lengthTolerance", 0.5).clamp(0.0, 1e6);
    let name = text(p, "lines");
    let lines = inp.dataset(name).ok_or_else(|| format!("no layer named '{name}'"))?;
    let nodes_ds = layer(inp, p, "nodes")?;
    let dims_ds = layer(inp, p, "dimensions")?;
    for d in [Some(lines), nodes_ds, dims_ds].into_iter().flatten() {
        if !lonlat(&d.extent()) {
            return Err(format!(
                "'{}' isn't in longitude/latitude: add the drawing with its coordinate system (\"crs\": EPSG:3405 for VN-2000 / UTM zone 48N, or the province's VN-2000 TM-3 +proj string)",
                d.name
            ));
        }
    }
    let ext = lines.extent();
    let plane = Aeqd::new(Coord::new((ext.min_x + ext.max_x) / 2.0, (ext.min_y + ext.max_y) / 2.0));
    let prior = match layer(inp, p, "proposals")? {
        Some(d) => Decisions::read(d, &plane)?,
        None => Decisions::default(),
    };
    let patterns: Vec<&str> = text(p, "layers").split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
    let mut capped = false;
    let runs = read_runs(inp, name, lines, &patterns, &plane, &mut capped);
    if runs.is_empty() {
        return Err(if patterns.is_empty() {
            format!("'{name}' has no lines")
        } else {
            format!("'{name}' has no lines on CAD layers '{}'", text(p, "layers"))
        });
    }
    let (nodes, node_fields) = read_nodes(nodes_ds, text(p, "nodeName"), &plane, &mut capped)?;
    let dims = read_dims(dims_ds, &plane, &mut capped)?;
    let anchors = nodes.iter().enumerate().map(|(k, n)| (n.at, k)).collect();
    let book = Book { found: Vec::new(), prior, tol: snap.max(0.05), dropped: 0 };
    let mut net = Net { runs, nodes, dims, snap, ask, book, work: 0, overshoots: Vec::new(), anchors, on: Vec::new() };
    net.duplicates();
    net.ends();
    net.mid_crossings();
    net.node_cuts();
    net.dimension_cuts();
    let network = net.network_layer(lines, nodes_ds, &node_fields, tolerance, &plane);
    let proposals = net.proposals_layer(lines, &plane);
    let messages = net.summary(network.features.len(), capped);
    Ok(ToolResult { output: Output::Layers(vec![OutputLayer::Vector(network), OutputLayer::Vector(proposals)]), messages })
}

impl Net {
    /// Lines drawn twice: on one CAD layer one is dropped; across CAD layers dropping is
    /// proposed.
    fn duplicates(&mut self) {
        for i in 0..self.runs.len() {
            for j in i + 1..self.runs.len() {
                self.work += 1;
                if self.work > MAX_WORK {
                    return;
                }
                let (Some(a), Some(b)) = (self.runs.get(i), self.runs.get(j)) else { continue };
                if a.gone || b.gone || !same_line(&a.pts, &b.pts) {
                    continue;
                }
                let level = if a.layer.eq_ignore_ascii_case(&b.layer) { Level::Fixed } else { Level::Ask };
                let (at, row, note) = (point_at(&b.pts, length(&b.pts) / 2.0), b.row, format!("drawn twice (also on {})", a.layer));
                if self.book.note("duplicate", at, None, Some(row), note, level)
                    && let Some(b) = self.runs.get_mut(j)
                {
                    b.gone = true;
                }
            }
        }
    }

    /// Every end: joined to a node or line it stops short of, trimmed where it runs past a
    /// line; an end with nothing near is reported.
    fn ends(&mut self) {
        for i in 0..self.runs.len() {
            for start in [true, false] {
                if self.work > MAX_WORK {
                    return;
                }
                self.end(i, start);
            }
        }
    }

    fn end(&mut self, i: usize, start: bool) {
        let Some(run) = self.runs.get(i).filter(|r| !r.gone) else { return };
        let Some(end) = (if start { run.pts.first() } else { run.pts.last() }).copied() else { return };
        let (row, len) = (run.row, length(&run.pts));
        let node = self.nodes.iter().map(|n| (dist(n.at, end), n.at)).min_by(|a, b| a.0.total_cmp(&b.0));
        if node.is_some_and(|(d, _)| d < SAME) {
            return;
        }
        // The nearest line it stops short of; the nearest it runs past (how far past, where
        // along this line, where).
        let mut short: Option<(f64, Coord)> = None;
        let mut past: Option<(f64, f64, Coord)> = None;
        for (j, other) in self.runs.iter().enumerate() {
            if j == i || other.gone {
                continue;
            }
            self.work += other.pts.len();
            if let Some((d, _, q)) = foot(&other.pts, end)
                && short.is_none_or(|(sd, _)| d < sd)
            {
                short = Some((d, q));
            }
            for (s, _, x) in crossings(&run.pts, &other.pts, &mut self.work) {
                let tail = if start { s } else { len - s };
                if tail > SAME && tail <= self.ask && past.is_none_or(|(t, ..)| tail < t) {
                    past = Some((tail, s, x));
                }
            }
        }
        let (snap, ask) = (self.snap, self.ask);
        let (kind, to, note, level, trim) = match (node.filter(|(d, _)| *d < snap), past) {
            (Some((d, at)), _) => ("gap", at, format!("stops {d:.2} m from a node"), Level::Fixed, None),
            (None, Some((tail, s, x))) => {
                self.overshoots.push(x);
                let level = if tail < snap { Level::Fixed } else { Level::Ask };
                ("overshoot", x, format!("runs {tail:.2} m past a line"), level, Some(s))
            }
            (None, None) => match [node, short].into_iter().flatten().filter(|(d, _)| *d <= ask).min_by(|a, b| a.0.total_cmp(&b.0)) {
                Some((d, _)) if d < SAME => return,
                Some((d, q)) => {
                    let level = if d < snap { Level::Fixed } else { Level::Ask };
                    ("gap", q, format!("stops {d:.2} m short of a line or node"), level, None)
                }
                None => {
                    self.book.note("dangling_end", end, None, Some(row), format!("an end with nothing within {ask} m"), Level::Report);
                    return;
                }
            },
        };
        if !self.book.note(kind, end, Some(to), Some(row), note, level) {
            return;
        }
        let Some(run) = self.runs.get_mut(i) else { return };
        match trim {
            Some(s) => {
                let mut pieces = split(&run.pts, &[s]);
                let kept = if start { pieces.pop() } else { pieces.into_iter().next() };
                if let Some((_, pts)) = kept {
                    run.pts = pts;
                }
            }
            None => {
                if let Some(p) = if start { run.pts.first_mut() } else { run.pts.last_mut() } {
                    *p = to;
                }
            }
        }
    }

    /// Lines crossing mid-line with no node: proposed to be cut there (they may be pipes at
    /// different depths).
    fn mid_crossings(&mut self) {
        for i in 0..self.runs.len() {
            for j in i + 1..self.runs.len() {
                if self.work > MAX_WORK {
                    return;
                }
                let (Some(a), Some(b)) = (self.runs.get(i), self.runs.get(j)) else { continue };
                if a.gone || b.gone {
                    continue;
                }
                let (la, lb, row) = (length(&a.pts), length(&b.pts), a.row);
                let found = crossings(&a.pts, &b.pts, &mut self.work);
                let snap = self.snap;
                for (sa, sb, x) in found {
                    let near = |p: Coord| dist(p, x) <= snap;
                    let at_end = sa < SAME || la - sa < SAME || sb < SAME || lb - sb < SAME;
                    if at_end || self.overshoots.iter().any(|p| near(*p)) || self.nodes.iter().any(|n| near(n.at)) {
                        continue;
                    }
                    if self.book.note(
                        "crossing",
                        x,
                        None,
                        Some(row),
                        "crosses another line with no node: pipes at different depths?".into(),
                        Level::Ask,
                    ) {
                        if let Some(a) = self.runs.get_mut(i) {
                            a.cuts.push(sa);
                        }
                        if let Some(b) = self.runs.get_mut(j) {
                            b.cuts.push(sb);
                        }
                    }
                }
            }
        }
    }

    /// Nodes on lines: a line is cut at a node on it; a cut at the foot of a node a little off
    /// a line is proposed; a node far from every line is reported.
    fn node_cuts(&mut self) {
        let vertices: usize = self.runs.iter().map(|r| r.pts.len()).sum();
        for (k, n) in self.nodes.iter().enumerate() {
            self.work += vertices;
            if self.work > MAX_WORK {
                return;
            }
            let best = self
                .runs
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.gone)
                .filter_map(|(i, r)| foot(&r.pts, n.at).map(|f| (i, f)))
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0));
            let Some((i, (d, s, q))) = best.filter(|(_, (d, ..))| *d <= self.ask) else {
                self.book.note(
                    "stray_node",
                    n.at,
                    None,
                    None,
                    format!("{} is more than {} m from every line", n.name, self.ask),
                    Level::Report,
                );
                continue;
            };
            let Some(run) = self.runs.get(i) else { continue };
            let (len, row) = (length(&run.pts), run.row);
            // At an end: the ends were joined already.
            if s < self.snap || len - s < self.snap {
                continue;
            }
            let cut = d < self.snap
                || self.book.note("node_off_line", n.at, Some(q), Some(row), format!("{} is {d:.2} m off the line", n.name), Level::Ask);
            if cut {
                if let Some(run) = self.runs.get_mut(i) {
                    run.cuts.push(s);
                }
                self.anchors.push((q, k));
            }
        }
    }

    /// Dimensions on lines: each end mid-line, where the written length starts or stops, is
    /// proposed as a cut; a dimension on no line is reported.
    fn dimension_cuts(&mut self) {
        let vertices: usize = self.runs.iter().map(|r| r.pts.len()).sum();
        for (k, dm) in self.dims.iter().enumerate() {
            self.work += 2 * vertices;
            if self.work > MAX_WORK {
                return;
            }
            let best = self
                .runs
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.gone)
                .filter_map(|(i, r)| {
                    let (da, sa, qa) = foot(&r.pts, dm.a)?;
                    let (db, sb, qb) = foot(&r.pts, dm.b)?;
                    Some((da.max(db), i, (sa, qa), (sb, qb)))
                })
                .filter(|(d, ..)| *d <= self.snap)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, i, (sa, qa), (sb, qb))) = best else {
                let mid = Coord::new((dm.a.x + dm.b.x) / 2.0, (dm.a.y + dm.b.y) / 2.0);
                self.book.note("stray_dimension", mid, None, None, format!("a {:.2} m dimension on no line", dm.measure), Level::Report);
                continue;
            };
            self.on.push((k, i, sa.min(sb), sa.max(sb)));
            for (s, q) in [(sa, qa), (sb, qb)] {
                let Some(run) = self.runs.get(i) else { continue };
                let (len, row) = (length(&run.pts), run.row);
                if s < self.snap || len - s < self.snap || run.cuts.iter().any(|c| (c - s).abs() < self.snap) {
                    continue;
                }
                let note = format!("a {:.2} m dimension ends here, mid-line", dm.measure);
                if self.book.note("cut_at_dimension", q, None, Some(row), note, Level::Ask)
                    && let Some(run) = self.runs.get_mut(i)
                {
                    run.cuts.push(s);
                }
            }
        }
    }

    /// The network: each line cut into its pieces, with the nodes at their ends, the length a
    /// dimension notes and the length drawn. Pieces whose length disagrees, or that lack a node
    /// or a dimension, are reported.
    fn network_layer(&mut self, lines: &Dataset, nodes: Option<&Dataset>, node_fields: &[usize], tolerance: f64, plane: &Aeqd) -> Dataset {
        let mut fields = lines.fields.clone();
        let base = fields.len();
        fields.push(Field::new("node1", FieldType::Text));
        fields.push(Field::new("node2", FieldType::Text));
        let copied: Vec<&Field> = node_fields.iter().filter_map(|i| nodes.and_then(|n| n.fields.get(*i))).collect();
        for prefix in ["n1_", "n2_"] {
            for f in &copied {
                let mut f = (*f).clone();
                f.name = format!("{prefix}{}", f.name);
                fields.push(f);
            }
        }
        for name in ["len_drawn", "len_noted", "len_diff"] {
            fields.push(Field::new(name, FieldType::Double));
        }
        let mut out = Dataset::new(format!("{} network", lines.name), GeometryType::Polyline, fields);
        out.crs = lines.crs;
        let reach = self.snap.max(SAME);
        let runs = std::mem::take(&mut self.runs);
        for (i, run) in runs.iter().enumerate().filter(|(_, r)| !r.gone) {
            let len = length(&run.pts);
            let mut cuts: Vec<f64> = run.cuts.iter().copied().filter(|c| *c > SAME && len - *c > SAME).collect();
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < SAME);
            let Some(attrs) = lines.features.get(run.row).map(|f| &f.attrs) else { continue };
            for (start, pts) in split(&run.pts, &cuts) {
                let end = start + length(&pts);
                let at_node = |p: Option<&Coord>| {
                    p.and_then(|p| {
                        self.anchors.iter().map(|(a, k)| (dist(*a, *p), *k)).filter(|(d, _)| *d <= reach).min_by(|a, b| a.0.total_cmp(&b.0))
                    })
                    .and_then(|(_, k)| self.nodes.get(k))
                };
                let ends = [at_node(pts.first()), at_node(pts.last())];
                let noted = self
                    .on
                    .iter()
                    .find(|(_, r, lo, hi)| *r == i && (lo - start).abs() < reach && (hi - end).abs() < reach)
                    .and_then(|(k, ..)| self.dims.get(*k))
                    .map(|d| d.measure);
                let ll: Vec<Coord> = pts.iter().map(|c| plane.inverse(*c)).collect();
                let drawn: f64 = ll
                    .windows(2)
                    .map(|w| match w {
                        [a, b] => wgs84_distance_m(*a, *b),
                        _ => 0.0,
                    })
                    .sum();
                let mut a = attrs.clone();
                a.resize(base, Value::Null);
                for n in ends {
                    a.push(n.map(|n| Value::Text(n.name.clone())).unwrap_or(Value::Null));
                }
                for n in ends {
                    for &fi in node_fields {
                        a.push(match (n, nodes) {
                            (Some(n), Some(ds)) => ds.value(n.row, fi).clone(),
                            _ => Value::Null,
                        });
                    }
                }
                a.push(Value::Float(drawn));
                a.push(noted.map(Value::Float).unwrap_or(Value::Null));
                a.push(noted.map(|n| Value::Float(drawn - n)).unwrap_or(Value::Null));
                out.push_next(Some(Geometry::Line(vec![ll])), a);
                let (mid, row, no_node) = (point_at(&pts, (end - start) / 2.0), Some(run.row), ends.iter().all(Option::is_none));
                if let Some(n) = noted
                    && (drawn - n).abs() > tolerance
                {
                    self.book.note(
                        "length_mismatch",
                        mid,
                        None,
                        row,
                        format!("drawn {drawn:.2} m, its dimension says {n:.2} m"),
                        Level::Report,
                    );
                }
                if !self.nodes.is_empty() && no_node {
                    self.book.note("piece_without_node", mid, None, row, "no node at either end".into(), Level::Report);
                }
                if !self.dims.is_empty() && noted.is_none() {
                    self.book.note(
                        "piece_without_dimension",
                        mid,
                        None,
                        row,
                        format!("{drawn:.2} m drawn; no dimension gives its length"),
                        Level::Report,
                    );
                }
            }
        }
        out
    }

    /// The findings as points: kind, note, confidence, decision, the input line's id.
    fn proposals_layer(&self, lines: &Dataset, plane: &Aeqd) -> Dataset {
        let mut fields: Vec<Field> =
            ["kind", "note", "confidence", "decision"].into_iter().map(|n| Field::new(n, FieldType::Text)).collect();
        fields.push(Field::new("line", FieldType::Integer));
        let mut out = Dataset::new(format!("{} proposals", lines.name), GeometryType::Point, fields);
        out.crs = lines.crs;
        for f in &self.book.found {
            let line =
                f.line.and_then(|r| lines.features.get(r)).and_then(|ft| i64::try_from(ft.id).ok()).map(Value::Int).unwrap_or(Value::Null);
            let attrs = vec![
                Value::Text(f.kind.into()),
                Value::Text(f.note.clone()),
                Value::Text(f.level.label().into()),
                Value::Text(f.decision.into()),
                line,
            ];
            out.push_next(Some(Geometry::Point(plane.inverse(f.at))), attrs);
        }
        out
    }

    fn summary(&self, pieces: usize, capped: bool) -> Vec<String> {
        let f = &self.book.found;
        let n = |level: Level, decision: &str| f.iter().filter(|x| x.level == level && x.decision == decision).count();
        let open = n(Level::Ask, "");
        let reported = f.iter().filter(|x| x.level == Level::Report).count();
        let mut m = vec![format!(
            "{pieces} pieces: {} fixes made, {} proposals accepted, {open} to decide, {reported} reported",
            n(Level::Fixed, "accept"),
            n(Level::Ask, "accept")
        )];
        if open > 0 {
            m.push("to decide: set 'decision' to accept or skip in the proposals layer, then run again with it as 'proposals'".into());
        }
        if self.book.dropped > 0 {
            m.push(format!("{} more findings weren't written: at most {MAX_FINDINGS}", self.book.dropped));
        }
        if self.work > MAX_WORK {
            m.push("the lines are too many to check whole: some ends, crossings and nodes weren't checked".into());
        }
        if capped {
            m.push(format!("more than {MAX_ITEMS} lines, nodes or dimensions: the rest were left out"));
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: f64, y: f64) -> Coord {
        Coord::new(x, y)
    }

    /// Where the test lines are: metres east and north of here.
    const AT: Coord = Coord { x: 105.9, y: 20.9 };

    /// A point `x`, `y` metres east and north of [`AT`], as longitude/latitude.
    fn m(x: f64, y: f64) -> Coord {
        Aeqd::new(AT).inverse(Coord::new(x, y))
    }

    struct Layers(Vec<Dataset>);
    impl Inputs for Layers {
        fn dataset(&self, layer: &str) -> Option<&Dataset> {
            self.0.iter().find(|d| d.name == layer)
        }
        fn selection(&self, _: &str) -> &[usize] {
            &[]
        }
    }

    fn lines(runs: &[(&str, &[(f64, f64)])]) -> Dataset {
        let mut d = Dataset::new("lines", GeometryType::Polyline, vec![Field::new("layer", FieldType::Text)]);
        for (layer, pts) in runs {
            d.push(Some(Geometry::Line(vec![pts.iter().map(|(x, y)| m(*x, *y)).collect()])), vec![Value::Text((*layer).into())]);
        }
        d
    }

    /// Manholes: named, with a level 3, 4, 5… in `TK`.
    fn nodes(ns: &[(&str, f64, f64)]) -> Dataset {
        let fields = vec![Field::new("layer", FieldType::Text), Field::new("TEN", FieldType::Text), Field::new("TK", FieldType::Double)];
        let mut d = Dataset::new("nodes", GeometryType::Point, fields);
        for (i, (name, x, y)) in ns.iter().enumerate() {
            d.push(
                Some(Geometry::Point(m(*x, *y))),
                vec![Value::Text("T-HO GA".into()), Value::Text((*name).into()), Value::Float(3.0 + i as f64)],
            );
        }
        d
    }

    /// Dimensions: from, to, the length they measure.
    fn dims(ds: &[((f64, f64), (f64, f64), f64)]) -> Dataset {
        let fields = vec![Field::new("entity", FieldType::Text), Field::new("measure", FieldType::Double)];
        let mut d = Dataset::new("dims", GeometryType::Polyline, fields);
        for (a, b, measure) in ds {
            d.push(
                Some(Geometry::Line(vec![vec![m(a.0, a.1), m(b.0, b.1)]])),
                vec![Value::Text("DIMENSION".into()), Value::Float(*measure)],
            );
        }
        d
    }

    /// Runs the tool: the network and the proposals.
    fn run(layers: Vec<Dataset>, p: Json) -> (Dataset, Dataset) {
        match network(&Layers(layers), &p).unwrap().output {
            Output::Layers(v) => match <[OutputLayer; 2]>::try_from(v) {
                Ok([OutputLayer::Vector(n), OutputLayer::Vector(p)]) => (n, p),
                v => panic!("{v:?}"),
            },
            o => panic!("{o:?}"),
        }
    }

    fn error(layers: Vec<Dataset>, p: Json) -> String {
        match network(&Layers(layers), &p) {
            Err(e) => e,
            Ok(r) => panic!("expected an error: {:?}", r.messages),
        }
    }

    /// The findings of a kind: (confidence, decision) each.
    fn found(props: &Dataset, kind: &str) -> Vec<(String, String)> {
        let f = |n: &str| props.field_index(n).unwrap();
        (0..props.features.len())
            .filter(|r| text_of(props.value(*r, f("kind"))) == kind)
            .map(|r| (text_of(props.value(r, f("confidence"))), text_of(props.value(r, f("decision")))))
            .collect()
    }

    /// The proposals with every one of `kind` decided `decision`, as layer `props`.
    fn decide(mut props: Dataset, kind: &str, decision: &str) -> Dataset {
        let (k, d) = (props.field_index("kind").unwrap(), props.field_index("decision").unwrap());
        for f in props.features.iter_mut() {
            if f.attrs[k] == Value::Text(kind.into()) {
                f.attrs[d] = Value::Text(decision.into());
            }
        }
        props.name = "props".into();
        props
    }

    fn number(d: &Dataset, row: usize, field: &str) -> f64 {
        d.value(row, d.field_index(field).unwrap()).as_f64().unwrap()
    }

    fn txt(d: &Dataset, row: usize, field: &str) -> String {
        text_of(d.value(row, d.field_index(field).unwrap()))
    }

    #[test]
    fn plane_helpers() {
        let l = [c(0.0, 0.0), c(60.0, 0.0), c(60.0, 40.0)];
        assert_eq!(length(&l), 100.0);
        assert_eq!(point_at(&l, 80.0), c(60.0, 20.0));
        assert_eq!(point_at(&l, 500.0), c(60.0, 40.0));
        assert_eq!(foot(&l, c(70.0, 10.0)), Some((10.0, 70.0, c(60.0, 10.0))));
        assert_eq!(cross(c(0.0, 0.0), c(10.0, 0.0), c(5.0, -5.0), c(5.0, 5.0)), Some((0.5, 0.5)));
        assert_eq!(cross(c(0.0, 0.0), c(10.0, 0.0), c(0.0, 1.0), c(10.0, 1.0)), None, "parallel");
        let mut work = 0;
        assert_eq!(crossings(&l, &[c(30.0, -5.0), c(30.0, 5.0)], &mut work), vec![(30.0, 5.0, c(30.0, 0.0))]);
        assert_eq!(work, 2);
        let pieces = split(&l, &[30.0, 60.0]);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], (0.0, vec![c(0.0, 0.0), c(30.0, 0.0)]));
        assert_eq!(pieces[1], (30.0, vec![c(30.0, 0.0), c(60.0, 0.0)]));
        assert_eq!(pieces[2], (60.0, vec![c(60.0, 0.0), c(60.0, 40.0)]));
        assert_eq!(split(&l, &[]).len(), 1);
        assert!(same_line(&l, &[c(60.0, 40.0), c(60.0, 0.0), c(0.0, 0.0)]));
        assert!(!same_line(&l, &l[..2]));
    }

    #[test]
    fn pieces_take_their_end_nodes_and_lengths() {
        let (net, props) = run(
            vec![
                lines(&[("ONG 300", &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0)]), ("ONG 500", &[(0.0, 100.0), (50.0, 100.0)])]),
                nodes(&[("A1", 0.0, 0.0), ("A2", 60.0, 40.0)]),
            ],
            serde_json::json!({"lines": "lines", "layers": "ONG 3*", "nodes": "nodes"}),
        );
        assert_eq!((net.name.as_str(), net.features.len()), ("lines network", 1), "only the CAD layers asked for");
        assert!((number(&net, 0, "len_drawn") - 100.0).abs() < 1e-3);
        assert_eq!((txt(&net, 0, "node1"), txt(&net, 0, "node2")), ("A1".into(), "A2".into()));
        assert_eq!((number(&net, 0, "n1_TK"), number(&net, 0, "n2_TK")), (3.0, 4.0));
        assert!(net.field_index("n1_layer").is_none(), "the drawing's own fields aren't copied");
        assert!(net.value(0, net.field_index("len_noted").unwrap()).is_null());
        assert_eq!((props.name.as_str(), props.features.len()), ("lines proposals", 0));
    }

    #[test]
    fn inputs_are_checked() {
        let base = || lines(&[("ONG 300", &[(0.0, 0.0), (10.0, 0.0)])]);
        let mut far = Dataset::new("lines", GeometryType::Polyline, vec![]);
        far.push(Some(Geometry::Line(vec![vec![Coord::new(397294.6, 2308552.3), Coord::new(397394.6, 2308552.3)]])), vec![]);
        assert!(error(vec![far], serde_json::json!({"lines": "lines"})).contains("crs"));
        let no_measure = Dataset::new("dims", GeometryType::Polyline, vec![Field::new("entity", FieldType::Text)]);
        assert!(error(vec![base(), no_measure], serde_json::json!({"lines": "lines", "dimensions": "dims"})).contains("'measure'"));
        let mut not_proposals = nodes(&[]);
        not_proposals.name = "props".into();
        assert!(
            error(vec![base(), not_proposals], serde_json::json!({"lines": "lines", "proposals": "props"})).contains("proposals layer")
        );
        assert!(
            error(vec![base(), nodes(&[])], serde_json::json!({"lines": "lines", "nodes": "nodes", "nodeName": "NAME"})).contains("'NAME'")
        );
        assert!(error(vec![base()], serde_json::json!({"lines": "lines", "layers": "DAT_*"})).contains("DAT_*"));
        assert!(error(vec![base()], serde_json::json!({"lines": "nope"})).contains("nope"));
        // Junk numbers fall back to the defaults or are clamped.
        let (net, _) = run(vec![base()], serde_json::json!({"lines": "lines", "snap": "-1", "ask": 1e308, "lengthTolerance": "x"}));
        assert_eq!(net.features.len(), 1);
    }

    #[test]
    fn gaps_are_fixed_proposed_or_reported() {
        let l = || {
            lines(&[
                ("ONG 300", &[(0.0, 0.0), (100.0, 0.0)]),
                ("ONG 300", &[(100.3, 0.0), (200.0, 0.0)]),
                ("ONG 300", &[(0.0, 50.0), (100.0, 50.0)]),
                ("ONG 300", &[(105.0, 50.0), (200.0, 50.0)]),
            ])
        };
        let (net, props) = run(vec![l()], serde_json::json!({"lines": "lines"}));
        assert_eq!(found(&props, "gap"), [("fixed".into(), "accept".into()), ("ask".into(), "".into())], "0.3 m fixed, 5 m asked once");
        assert_eq!(found(&props, "dangling_end").len(), 4);
        assert!((number(&net, 0, "len_drawn") - 100.3).abs() < 1e-3, "the first line reaches the second");
        assert!((number(&net, 2, "len_drawn") - 100.0).abs() < 1e-3, "the 5 m gap waits for a decision");
        let props = decide(props, "gap", "accept");
        let (net, again) = run(vec![l(), props], serde_json::json!({"lines": "lines", "proposals": "props"}));
        assert!((number(&net, 2, "len_drawn") - 105.0).abs() < 1e-3, "accepted: joined");
        assert_eq!(found(&again, "gap"), [("fixed".into(), "accept".into()), ("ask".into(), "accept".into())]);
        // Skipping a certain fix undoes it.
        let props = decide(again, "gap", "skip");
        let (net, _) = run(vec![l(), props], serde_json::json!({"lines": "lines", "proposals": "props"}));
        assert!((number(&net, 0, "len_drawn") - 100.0).abs() < 1e-3);
    }

    #[test]
    fn overshoots_are_trimmed_or_proposed() {
        let (net, props) = run(
            vec![lines(&[
                ("ONG 300", &[(0.0, 200.0), (100.0, 200.0)]),
                ("ONG 300", &[(50.0, 250.0), (50.0, 199.7)]),
                ("ONG 300", &[(80.0, 250.0), (80.0, 197.0)]),
            ])],
            serde_json::json!({"lines": "lines"}),
        );
        assert_eq!(found(&props, "overshoot"), [("fixed".into(), "accept".into()), ("ask".into(), "".into())]);
        assert!((number(&net, 1, "len_drawn") - 50.0).abs() < 1e-3, "trimmed where it crosses");
        assert!((number(&net, 2, "len_drawn") - 53.0).abs() < 1e-3);
    }

    #[test]
    fn duplicates_are_dropped_or_proposed() {
        let (net, props) = run(
            vec![lines(&[
                ("ONG 300", &[(0.0, 0.0), (100.0, 0.0)]),
                ("ong 300", &[(100.0, 0.0), (0.0, 0.0)]),
                ("ONG 300", &[(0.0, 50.0), (100.0, 50.0)]),
                ("ONG 500", &[(0.0, 50.0), (100.0, 50.0)]),
            ])],
            serde_json::json!({"lines": "lines"}),
        );
        assert_eq!(found(&props, "duplicate"), [("fixed".into(), "accept".into()), ("ask".into(), "".into())]);
        assert_eq!(net.features.len(), 3);
    }

    #[test]
    fn crossings_mid_line_are_proposed() {
        let l = || {
            lines(&[
                ("ONG 300", &[(0.0, 0.0), (100.0, 0.0)]),
                ("ONG 500", &[(50.0, -50.0), (50.0, 50.0)]),
                ("ONG 300", &[(0.0, 200.0), (100.0, 200.0)]),
                ("ONG 300", &[(80.0, 250.0), (80.0, 197.0)]),
            ])
        };
        let (net, props) = run(vec![l()], serde_json::json!({"lines": "lines"}));
        assert_eq!(found(&props, "crossing"), [("ask".into(), "".into())], "an overshoot isn't a crossing");
        assert_eq!(net.features.len(), 4);
        let (net, _) = run(vec![l(), decide(props, "crossing", "accept")], serde_json::json!({"lines": "lines", "proposals": "props"}));
        assert_eq!(net.features.len(), 6, "both lines cut where they cross");
    }

    #[test]
    fn lines_are_cut_at_nodes() {
        let l = || lines(&[("ONG 300", &[(0.0, 0.0), (100.0, 0.0)])]);
        let n = || nodes(&[("A0", 0.0, 0.0), ("A1", 40.0, 0.2), ("A2", 70.0, 3.0), ("A3", 50.0, 80.0), ("A4", 100.0, 0.0)]);
        let p = serde_json::json!({"lines": "lines", "nodes": "nodes"});
        let (net, props) = run(vec![l(), n()], p.clone());
        assert_eq!(net.features.len(), 2, "cut at the node 0.2 m off");
        assert_eq!((txt(&net, 0, "node1"), txt(&net, 0, "node2"), txt(&net, 1, "node2")), ("A0".into(), "A1".into(), "A4".into()));
        assert!((number(&net, 0, "len_drawn") - 40.0).abs() < 1e-3);
        assert_eq!(found(&props, "node_off_line"), [("ask".into(), "".into())]);
        assert_eq!(found(&props, "stray_node").len(), 1);
        let mut p = p;
        p["proposals"] = "props".into();
        let (net, _) = run(vec![l(), n(), decide(props, "node_off_line", "accept")], p);
        assert_eq!(net.features.len(), 3);
        assert_eq!((txt(&net, 1, "node2"), txt(&net, 2, "node1")), ("A2".into(), "A2".into()));
        assert_eq!(number(&net, 2, "n1_TK"), 5.0);
    }

    /// QH12's case: an ONG 500 run drawn 195.87 m between two manholes, its dimension 130.29 m
    /// from the first, both ends 0.1 m off the run.
    #[test]
    fn a_dimension_ending_mid_line_cuts_it_when_accepted() {
        let layers = || {
            vec![
                lines(&[("ONG 500", &[(0.0, 0.0), (195.87, 0.0)])]),
                nodes(&[("A1", 0.0, 0.0), ("A2", 195.87, 0.0)]),
                dims(&[((0.0, 0.1), (130.29, 0.1), 130.29)]),
            ]
        };
        let p = serde_json::json!({"lines": "lines", "nodes": "nodes", "dimensions": "dims"});
        let (net, props) = run(layers(), p.clone());
        assert_eq!(net.features.len(), 1);
        assert!(net.value(0, net.field_index("len_noted").unwrap()).is_null(), "the dimension is part of the run only");
        assert_eq!(found(&props, "cut_at_dimension"), [("ask".into(), "".into())], "its start is at a manhole already");
        assert_eq!(found(&props, "piece_without_dimension").len(), 1);

        let mut p = p;
        p["proposals"] = "props".into();
        let mut l = layers();
        l.push(decide(props, "cut_at_dimension", "accept"));
        let (net, props) = run(l, p);
        assert_eq!(net.features.len(), 2);
        assert_eq!(number(&net, 0, "len_noted"), 130.29);
        assert!(number(&net, 0, "len_diff").abs() < 1e-3);
        assert_eq!((txt(&net, 0, "node1"), txt(&net, 0, "node2"), txt(&net, 1, "node2")), ("A1".into(), "".into(), "A2".into()));
        assert!((number(&net, 1, "len_drawn") - 65.58).abs() < 1e-3);
        assert_eq!(found(&props, "cut_at_dimension"), [("ask".into(), "accept".into())]);
        assert_eq!(found(&props, "length_mismatch").len(), 0);
    }

    #[test]
    fn lengths_that_disagree_are_reported() {
        let (net, props) = run(
            vec![lines(&[("ONG 300", &[(0.0, 0.0), (90.0, 0.0)])]), dims(&[((0.0, 0.0), (90.0, 0.0), 70.0)])],
            serde_json::json!({"lines": "lines", "dimensions": "dims"}),
        );
        assert!((number(&net, 0, "len_diff") - 20.0).abs() < 1e-3);
        assert_eq!(found(&props, "length_mismatch").len(), 1);
    }

    #[test]
    fn a_dimension_on_no_line_is_reported() {
        let (_, props) = run(
            vec![lines(&[("ONG 300", &[(0.0, 0.0), (90.0, 0.0)])]), dims(&[((0.0, 30.0), (90.0, 30.0), 90.0)])],
            serde_json::json!({"lines": "lines", "dimensions": "dims"}),
        );
        assert_eq!(found(&props, "stray_dimension").len(), 1);
    }
}
