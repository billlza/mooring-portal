//! The sky of the drawn image: its light, its stars, its thin cloud, and
//! the far shore under it with a town's lamps at the water. Each part is
//! the part of the same name in the live scene's `shader-sky.mjs`, as it
//! stands when the scene's clock reads nothing.

use std::f64::consts::TAU;

use super::field::{cloudy, far_crest, hash, mix, near_crest, smoothstep};
use super::{
    AXIS, CLOUD, FAR_DEEP, FAR_HIGH, HAZE, NEAR_DARK, NEAR_PALE, Rgb, SEA, SKY_LOW, SKY_MID,
    SKY_RIM, SKY_TOP, STAR_COOL, STAR_WARM, TOWN_PALE, TOWN_WARM, WHITE, WIDTH, dim, gradient, n,
    polygon, rect,
};

/// How far the haze reaches, in the lengths it falls away over.
const HAZE_REACH: f64 = 2.2;
const HAZE_LENGTHS: (f64, f64) = (383.0, 180.0);
const HAZE_HEIGHT: f64 = 500.0;

/// How much of the low haze about the lighthouse lies over a place.
pub(super) fn veil(x: f64, y: f64) -> f64 {
    let (dx, dy) = (
        (x - AXIS) / HAZE_LENGTHS.0,
        (y - HAZE_HEIGHT) / HAZE_LENGTHS.1,
    );
    0.95 * (-(dx * dx + dy * dy)).exp()
}

/// Thin cloud: long streaks to the right of the sky.
fn wisps(x: f64, y: f64) -> f64 {
    let band = smoothstep(215.0, 300.0, y) * (1.0 - smoothstep(420.0, 500.0, y));
    let side = smoothstep(AXIS - 640.0, AXIS - 150.0, x);
    if band * side <= 0.0 {
        return 0.0;
    }
    smoothstep(0.5, 0.8, cloudy(x / 310.0, y / 40.0)) * band * side
}

/// How much of a star shows through the haze and the cloud at a place.
fn clear(x: f64, y: f64) -> f64 {
    (1.0 - veil(x, y))
        * (1.0 - 0.85 * wisps(x, y))
        * (1.0 - 0.75 * smoothstep(SEA - 110.0, SEA - 10.0, y))
}

pub(super) fn draw(out: &mut String) {
    out.push_str("<defs>");
    gradient(
        out,
        &format!(
            "linearGradient id=\"sky\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"{SEA}\""
        ),
        &[
            (0.0, SKY_TOP, 1.0),
            (0.52, SKY_MID, 1.0),
            (0.7, SKY_LOW, 1.0),
            (1.0, SKY_RIM, 1.0),
        ],
    );
    haze_paint(out, "haze");
    gradient(
        out,
        "radialGradient id=\"halo\"",
        &[
            (0.0, WHITE, 0.2),
            (0.27, WHITE, 0.133),
            (0.55, WHITE, 0.039),
            (1.0, WHITE, 0.0),
        ],
    );
    gradient(
        out,
        "linearGradient id=\"far\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"458\" x2=\"0\" y2=\"530\"",
        &[(0.0, FAR_HIGH, 1.0), (1.0, FAR_DEEP, 1.0)],
    );
    let across: Vec<(f64, Rgb, f64)> = (0..=4)
        .map(|step| {
            let t = f64::from(step) / 4.0;
            (t, NEAR_PALE.mix(NEAR_DARK, smoothstep(0.0, 1.0, t)), 1.0)
        })
        .collect();
    gradient(
        out,
        &format!(
            "linearGradient id=\"near\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"0\"",
            AXIS - 60.0,
            AXIS + 130.0
        ),
        &across,
    );
    gradient(
        out,
        "linearGradient id=\"deep\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"450\" x2=\"0\" y2=\"545\"",
        &[(0.0, NEAR_DARK, 0.0), (1.0, NEAR_DARK, 0.5)],
    );
    out.push_str(&format!(
        "<filter id=\"mist\" x=\"-10%\" y=\"-40%\" width=\"120%\" height=\"180%\">\
         <feGaussianBlur stdDeviation=\"26 8\"/></filter>\
         <clipPath id=\"above\"><rect width=\"{WIDTH}\" height=\"{SEA}\"/></clipPath></defs>"
    ));
    rect(out, (0.0, 0.0), (WIDTH, SEA + 1.0), "url(#sky)", "");
    haze(out, HAZE_HEIGHT, " clip-path=\"url(#above)\"");
    clouds(out);
    stars(out);
    hills(out);
    for lamp in lamps() {
        lamp.draw(out);
    }
}

/// The haze's gradient, under the name `id`: it falls away as the shader's
/// does, to nothing at [`HAZE_REACH`] lengths.
pub(super) fn haze_paint(out: &mut String, id: &str) {
    let stops: Vec<(f64, Rgb, f64)> = (0..=10)
        .map(|step| {
            let offset = f64::from(step) / 10.0;
            let lengths = offset * HAZE_REACH;
            let left = if step == 10 {
                0.0
            } else {
                0.95 * (-lengths * lengths).exp()
            };
            (offset, HAZE, left)
        })
        .collect();
    gradient(out, &format!("radialGradient id=\"{id}\""), &stops);
}

/// The haze about the lighthouse, its middle at the height `y`.
pub(super) fn haze(out: &mut String, y: f64, more: &str) {
    out.push_str(&format!(
        "<ellipse cx=\"{AXIS}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"url(#haze)\"{more}/>",
        n(y),
        n(HAZE_LENGTHS.0 * HAZE_REACH),
        n(HAZE_LENGTHS.1 * HAZE_REACH)
    ));
}

/// The thin cloud, as soft patches where the shader's cloud lies.
fn clouds(out: &mut String) {
    const PATCH: (f64, f64) = (64.0, 16.0);
    out.push_str(&format!(
        "<g fill=\"{}\" filter=\"url(#mist)\" clip-path=\"url(#above)\">",
        CLOUD.hex()
    ));
    let mut y = 215.0;
    while y < 500.0 {
        let mut x = AXIS - 640.0;
        while x < WIDTH {
            let cloud = wisps(x + PATCH.0 / 2.0, y + PATCH.1 / 2.0);
            if cloud > 0.04 {
                rect(
                    out,
                    (x, y),
                    (x + PATCH.0, y + PATCH.1),
                    "inherit",
                    &format!(" opacity=\"{}\"", dim(0.5 * cloud)),
                );
            }
            x += PATCH.0;
        }
        y += PATCH.1;
    }
    out.push_str("</g>");
}

/// The stars, one to a cell of the sky where there is one. The fainter
/// stars between them are the live scene's alone.
fn stars(out: &mut String) {
    const CELL: f64 = 46.0;
    let mut tints = [String::new(), String::new(), String::new()];
    let mut halos = String::new();
    let (columns, rows) = ((WIDTH / CELL).ceil() as i32, (SEA / CELL).ceil() as i32);
    for row in 0..rows {
        for column in 0..columns {
            let cell = (column, row);
            if hash(cell, 1) > 0.7 {
                continue;
            }
            let x = (f64::from(column) + 0.1 + 0.8 * hash(cell, 2)) * CELL;
            let y = (f64::from(row) + 0.1 + 0.8 * hash(cell, 3)) * CELL;
            let size = hash(cell, 4);
            let radius = 0.3 + 1.0 * size * size * size;
            let tint = hash(cell, 5);
            let phase = TAU * hash(cell, 7);
            let twinkle =
                0.6 + 0.4 * phase.sin() + 0.35 * smoothstep(0.82, 1.0, (phase * 3.0).sin());
            let clear = clear(x, y);
            let bright = (mix(0.4, 1.0, hash(cell, 8)) * twinkle).min(1.0) * clear;
            let place = format!("cx=\"{}\" cy=\"{}\"", n(x), n(y));
            if bright >= 0.03 {
                let group = if tint < 0.14 {
                    0
                } else if tint < 0.24 {
                    1
                } else {
                    2
                };
                tints[group].push_str(&format!(
                    "<circle {place} r=\"{radius:.2}\" opacity=\"{}\"/>",
                    dim(bright)
                ));
            }
            // One star in twelve has a soft halo.
            if hash(cell, 9) < 0.08 && twinkle * clear >= 0.1 {
                halos.push_str(&format!(
                    "<circle {place} r=\"11\" opacity=\"{}\"/>",
                    dim(twinkle * clear)
                ));
            }
        }
    }
    out.push_str(&format!("<g fill=\"url(#halo)\">{halos}</g>"));
    for (colour, stars) in [STAR_COOL, STAR_WARM, WHITE].into_iter().zip(tints) {
        out.push_str(&format!("<g fill=\"{}\">{stars}</g>", colour.hex()));
    }
}

/// The far range and the nearer hill, standing dark against the sky's rim.
fn hills(out: &mut String) {
    const STEP: f64 = 8.0;
    let line = |crest: fn(f64) -> f64, from: f64| {
        let mut shape = vec![(from, SEA + 2.0)];
        let mut x = from;
        while x < WIDTH {
            shape.push((x, crest(x)));
            x += STEP;
        }
        shape.push((WIDTH, crest(WIDTH)));
        shape.push((WIDTH, SEA + 2.0));
        shape
    };
    polygon(out, &line(far_crest, 0.0), "url(#far)", "");
    // Left of here the nearer hill lies under the water.
    let near = line(near_crest, 2260.0);
    polygon(out, &near, "url(#near)", "");
    polygon(out, &near, "url(#deep)", "");
}

/// How likely a lamp is at `x` along the far shore: a town under the far
/// range's highest part, and a few houses along the coast before it.
fn settled(x: f64) -> f64 {
    let town = smoothstep(2170.0, 2200.0, x) * (1.0 - smoothstep(2330.0, 2362.0, x));
    let coast = smoothstep(1930.0, 1960.0, x) * (1.0 - smoothstep(2120.0, 2170.0, x));
    0.8 * town + 0.16 * coast
}

/// One of the town's lamps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Lamp {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) colour: Rgb,
    pub(super) bright: f64,
}

impl Lamp {
    fn draw(&self, out: &mut String) {
        let place = format!(
            "cx=\"{}\" cy=\"{}\" fill=\"{}\"",
            n(self.x),
            n(self.y),
            self.colour.hex()
        );
        out.push_str(&format!(
            "<circle {place} r=\"4.2\" opacity=\"{}\"/><circle {place} r=\"1.3\" opacity=\"{}\"/>",
            dim(0.22 * self.bright),
            dim(self.bright)
        ));
    }
}

/// The town's lamps along the far shore, from left to right.
pub(super) fn lamps() -> Vec<Lamp> {
    const CELL: f64 = 6.0;
    let (first, last) = ((1930.0 / CELL) as i32, (2362.0 / CELL) as i32);
    (first..=last)
        .filter_map(|column| {
            let cell = (column, 5);
            let x = (f64::from(column) + hash(cell, 41)) * CELL;
            if hash(cell, 42) > settled(x) {
                return None;
            }
            let up = hash(cell, 43);
            let flicker = 0.86 + 0.14 * (TAU * hash(cell, 45)).sin();
            Some(Lamp {
                x,
                y: SEA - 2.5 - 15.0 * up * up,
                colour: TOWN_WARM.mix(TOWN_PALE, hash(cell, 46)),
                bright: flicker * mix(0.35, 1.0, hash(cell, 47)),
            })
        })
        .collect()
}
