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
    a.len() == b.len() && (a.iter().zip(b).all(|(p, q)| dist(*p, *q) < SAME) || a.iter().zip(b.iter().rev()).all(|(p, q)| dist(*p, *q) < SAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: f64, y: f64) -> Coord {
        Coord::new(x, y)
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
}
