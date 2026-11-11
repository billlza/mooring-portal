//! The sea of the drawn image: dark water that mirrors the far shore and
//! the haze over it, lies darker under the near shore, and carries the
//! town's lamps and the lighthouse's across its ripples. It is the live
//! scene's `shader-water.mjs` as it stands when the scene's clock reads
//! nothing, and still: the glitter that comes and goes there is here a
//! scattering of the same light.

use std::f64::consts::TAU;

use super::field::{along, far_crest, hash, mix, near_crest, smoothstep};
use super::shore::{pier_foot, pier_top};
use super::sky::{haze, lamps};
use super::{
    AXIS, FAR_DEEP, GOLD, GOLD_DEEP, HEIGHT, NEAR_DARK, PIER, ROCKS, SEA, SEA_DEEP, SEA_FAR,
    SEA_MID, SEA_SHADE, SEA_SHEEN, STRAND, WIDTH, dim, gradient, n, polygon, rect,
};

/// How long a ripple is at a depth: 0 at the far waterline, 1 at the
/// picture's foot.
fn ripple(depth: f64) -> f64 {
    1.4 + 8.5 * depth
}

/// The height at which the ripples' row `row`, counted from the far
/// shore, begins.
fn height(row: f64) -> f64 {
    SEA + ((row / 28.8).exp() - 1.4) / 8.5 * (HEIGHT - SEA)
}

/// The row a height falls in.
fn row(y: f64) -> f64 {
    28.8 * ripple((y - SEA) / (HEIGHT - SEA)).ln()
}

pub(super) fn draw(out: &mut String) {
    out.push_str("<defs>");
    gradient(
        out,
        &format!(
            "linearGradient id=\"sea\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"{SEA}\" x2=\"0\" y2=\"{HEIGHT}\""
        ),
        &[
            (0.0, SEA_FAR, 1.0),
            (0.3, SEA_MID, 1.0),
            (1.0, SEA_DEEP, 1.0),
        ],
    );
    // Far water mirrors nearly all it faces; near water shows its own dark.
    gradient(
        out,
        &format!(
            "linearGradient id=\"mirror\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"{SEA}\" x2=\"0\" y2=\"{}\"",
            SEA + 0.8 * (HEIGHT - SEA)
        ),
        &[
            (0.0, SEA_SHADE, 0.0),
            (0.5, SEA_SHADE, 0.45),
            (1.0, SEA_SHADE, 0.9),
        ],
    );
    gradient(
        out,
        &format!(
            "linearGradient id=\"lee\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"{STRAND}\" x2=\"0\" y2=\"{}\"",
            STRAND + 150.0
        ),
        &[
            (0.0, SEA_SHADE, 0.72),
            (0.33, SEA_SHADE, 0.35),
            (0.66, SEA_SHADE, 0.17),
            (1.0, SEA_SHADE, 0.08),
        ],
    );
    out.push_str(&format!(
        "<clipPath id=\"below\"><rect y=\"{SEA}\" width=\"{WIDTH}\" height=\"{}\"/></clipPath>\
         <mask id=\"far-water\"><rect y=\"{SEA}\" width=\"{WIDTH}\" height=\"{}\" fill=\"#ffffff\"/>\
         <rect y=\"{SEA}\" width=\"{WIDTH}\" height=\"{}\" fill=\"url(#mirror)\"/></mask></defs>",
        HEIGHT - SEA,
        HEIGHT - SEA,
        HEIGHT - SEA
    ));
    rect(out, (0.0, SEA), (WIDTH, HEIGHT), "url(#sea)", "");
    // What the water mirrors of the sky and the far shore, fainter with
    // every ripple toward the viewer.
    out.push_str("<g mask=\"url(#far-water)\" opacity=\"0.8\">");
    haze(out, 2.0 * SEA - 500.0, "");
    for (crest, colour, from) in [
        (far_crest as fn(f64) -> f64, FAR_DEEP, 0.0),
        (near_crest, NEAR_DARK, 2260.0),
    ] {
        let mut shape = vec![(from, SEA)];
        let mut x = from;
        while x <= WIDTH {
            shape.push((x, 2.0 * SEA - crest(x)));
            x += 16.0;
        }
        shape.push((WIDTH, SEA));
        polygon(out, &shape, &colour.hex(), "");
    }
    out.push_str("</g>");
    // The town's lamps, drawn out down the water.
    out.push_str("<g clip-path=\"url(#below)\">");
    for lamp in lamps() {
        let under = (SEA - lamp.y) / 0.16;
        out.push_str(&format!(
            "<ellipse cx=\"{}\" cy=\"{}\" rx=\"1.2\" ry=\"11\" fill=\"{}\" opacity=\"{}\"/>",
            n(lamp.x),
            n(SEA + under),
            lamp.colour.hex(),
            dim(0.7 * lamp.bright * (-under / 22.0).exp())
        ));
    }
    out.push_str("</g>");
    lee(out);
    glitter(out);
    lamp_path(out);
}

/// The near shore's dark in the water at its foot: its rocks and its pier,
/// mirrored there.
fn lee(out: &mut String) {
    let (from, to) = (-313.0, WIDTH - AXIS);
    let foot = |x: f64| if x > 77.0 { pier_foot(x) } else { STRAND };
    let top = |x: f64| {
        if x > 77.0 {
            pier_top(x)
        } else {
            along(ROCKS, x).min(if x > PIER { pier_top(x) } else { STRAND })
        }
    };
    let mut shape = Vec::new();
    let mut x = from;
    while x < to {
        shape.push((AXIS + x, foot(x)));
        x += 8.0;
    }
    shape.push((AXIS + to, foot(to)));
    while x > from {
        x -= 8.0;
        shape.push((AXIS + x, 2.0 * foot(x) - top(x)));
    }
    polygon(out, &shape, "url(#lee)", "");
}

/// Flecks of the haze's light on the ripples, most under the lighthouse.
fn glitter(out: &mut String) {
    out.push_str(&format!("<g fill=\"{}\">", SEA_SHEEN.hex()));
    for fleck in 0..420 {
        let id = (fleck, 7);
        let x = AXIS - 760.0 + 1400.0 * hash(id, 201);
        let line = 8.0 + (48.0 * hash(id, 202)).floor();
        let (y, tall) = (height(line + 0.5), height(line + 1.0) - height(line));
        let depth = (y - SEA) / (HEIGHT - SEA);
        let aside = (x - AXIS) / 430.0;
        let lit = (-aside * aside).exp() * (0.3 + 0.7 * smoothstep(0.02, 0.3, depth));
        // None in the near shore's dark.
        let shaded = (-320.0..600.0).contains(&(x - AXIS)) && y < STRAND + 46.0;
        if shaded || hash(id, 203) > 0.12 + 0.88 * lit {
            continue;
        }
        let long = ripple(depth) * 3.0 * mix(0.2, 0.54, hash(id, 204));
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{:.2}\" rx=\"{:.2}\" opacity=\"{}\"/>",
            n(x - long),
            n(y - 0.13 * tall),
            n(2.0 * long),
            0.26 * tall,
            0.13 * tall,
            dim((0.05 + 0.55 * lit) * mix(0.35, 1.0, hash(id, 205)))
        ));
    }
    out.push_str("</g>");
}

/// The lamp's path on the water, from the rocks under the lighthouse to
/// the viewer: a column of bright dashes, one to a ripple, with a smaller
/// glint beside some.
fn lamp_path(out: &mut String) {
    let (first, last) = (row(STRAND).ceil() as i32, row(HEIGHT).floor() as i32);
    for line in first..last {
        let id = (line, 9);
        let (top, foot) = (height(f64::from(line)), height(f64::from(line + 1)));
        let (y, tall) = ((top + foot) / 2.0, foot - top);
        let under = y - STRAND;
        let wide = 9.0 + 0.17 * under;
        let beat = (TAU * hash(id, 92)).sin();
        let reach = wide * (mix(0.3, 1.0, hash(id, 93)) + 0.22 * beat);
        let off = wide * 0.4 * (hash(id, 94) - 0.5 + 0.4 * (TAU * hash(id, 95)).sin());
        let thick = mix(0.26, 0.5, hash(id, 97));
        let strength = smoothstep(2.0, 26.0, under) * (1.0 - 0.6 * smoothstep(80.0, 200.0, under));
        let bright = strength * mix(0.5, 1.0, hash(id, 96));
        if bright < 0.03 {
            continue;
        }
        let colour = GOLD_DEEP.mix(GOLD, (1.4 * bright).min(1.0)).hex();
        let mut dash = |middle: f64, half: f64, deep: f64, opacity: f64| {
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{:.2}\" rx=\"{:.2}\" fill=\"{colour}\" opacity=\"{}\"/>",
                n(AXIS + middle - half),
                n(y - deep * tall),
                n(2.0 * half),
                2.0 * deep * tall,
                deep * tall,
                dim(opacity)
            ));
        };
        dash(off, 0.82 * reach, thick - 0.06, 0.8 * bright);
        let glint = smoothstep(0.1, 0.9, beat);
        if glint > 0.05 {
            let side = if hash(id, 99) < 0.5 { -1.0 } else { 1.0 };
            let aside = off + side * wide * mix(0.9, 1.7, hash(id, 98));
            dash(aside, 0.22 * wide, 0.16, 0.48 * bright * glint);
        }
    }
}
