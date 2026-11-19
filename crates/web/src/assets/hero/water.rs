//! The sea of the drawn image: dark water that mirrors the far shore and
//! the haze over it, holds the sky's light along its ripples, lies darker
//! under the near shore, and carries the town's lamps and the lighthouse's
//! across it. It is the live scene's `shader-water.mjs` as it stands when
//! the scene's clock reads nothing, and still: the points of light that
//! flash there are here a scattering of the same light.

use super::field::{along, hash, mix, noise, smoothstep};
use super::shore::{pier_foot, pier_top};
use super::sky::{RANGES, haze, lamps, land_haze};
use super::{
    AXIS, GOLD, GOLD_DEEP, HEIGHT, PIER, ROCKS, SEA, SEA_DEEP, SEA_FAR, SEA_MID, SEA_SHADE,
    SEA_SHEEN, STRAND, WIDTH, dim, gradient, n, polygon, rect,
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

/// A place on the water in the ripples' own measure: along its row, in
/// ripples from the lighthouse's axis, and in rows from the far shore.
fn place(x: f64, y: f64) -> (f64, f64) {
    ((x - AXIS) / ripple((y - SEA) / (HEIGHT - SEA)), row(y))
}

/// The water's face at a place: ripples many times as long as they are
/// tall, in two sizes, none of them level from end to end.
fn ripples(at: (f64, f64)) -> f64 {
    let bend = 0.45 * (at.0 / 17.0 + at.1 * 0.8).sin();
    0.62 * noise(at.0 / 22.0, at.1 * 1.5 + bend)
        + 0.38 * noise(at.0 / 8.0, at.1 * 3.3 + 5.0 - 1.7 * bend)
}

/// How far a row slides what it mirrors to one side, in ripples.
fn sway((across, row): (f64, f64)) -> f64 {
    0.6 * (row * 1.9 + across * 0.12).sin()
        + 0.3 * (row * 4.3 + across * 0.35).sin()
        + 0.5 * (noise(across * 0.16, row * 0.6) - 0.5)
}

/// How much of the sky's light the water holds at a place: more where a
/// ripple turns its face to the sky, where the swell opens the water to
/// it, and under the haze around the lighthouse.
fn sheen(x: f64, y: f64) -> f64 {
    let depth = (y - SEA) / (HEIGHT - SEA);
    let at = place(x, y);
    let face = ripples(at);
    let held = smoothstep(0.44, 0.84, face);
    let open = smoothstep(0.25, 0.8, noise(x / 380.0, at.1 / 6.0));
    let aside = (x - AXIS) / 430.0;
    let glow = (-aside * aside).exp();
    held * (0.25 + 0.75 * open)
        * (0.3 + 0.7 * glow)
        * mix(0.2, 0.09, depth)
        * smoothstep(0.0, 0.04, depth)
}

/// How brightly the lamp shows at a place on the water. Under the lamp
/// nearly every ripple shows it; to either side only one that tilts far
/// enough toward it does.
fn lamp_shows(x: f64, y: f64) -> f64 {
    let under = y - STRAND;
    let at = place(x, y);
    let wide = 9.0 + 0.17 * under;
    let ripple = ripple((y - SEA) / (HEIGHT - SEA));
    let off = (x - AXIS - sway(at) * ripple * 0.6).abs() / wide;
    let need = mix(0.3, 0.92, smoothstep(0.0, 1.3, off));
    let bend = 0.4 * (at.0 / 6.0 + at.1 * 1.3).sin();
    let tilt = 0.6 * noise(at.0 / 4.5, at.1 * 2.4 + 3.0 + bend)
        + 0.4 * noise(at.0 / 9.0, at.1 * 1.1 + 11.0 - bend);
    let strength = smoothstep(2.0, 26.0, under) * (1.0 - 0.6 * smoothstep(80.0, 200.0, under));
    smoothstep(need, need + 0.3, tilt) * strength
}

/// The stretches of a level line, from `from` to `to` in steps of `step`,
/// along which `value` is at least `least`: each with its two ends and
/// the mean of the value along it.
fn runs(
    (from, to): (f64, f64),
    step: f64,
    least: f64,
    value: impl Fn(f64) -> f64,
) -> Vec<(f64, f64, f64)> {
    let mut found = Vec::new();
    let mut open: Option<(f64, f64, f64)> = None;
    let mut x = from;
    while x <= to {
        let here = value(x);
        open = match open {
            Some((start, total, count)) if here >= least => {
                Some((start, total + here, count + 1.0))
            }
            Some((start, total, count)) => {
                found.push((start, x - step, total / count));
                None
            }
            None if here >= least => Some((x, here, 1.0)),
            None => None,
        };
        x += step;
    }
    if let Some((start, total, count)) = open {
        found.push((start, x - step, total / count));
    }
    found
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
    // every ripple toward the viewer: the haze, and the land of the far
    // shore turned over at the waterline.
    out.push_str("<g mask=\"url(#far-water)\" opacity=\"0.8\">");
    haze(out, 2.0 * SEA - 500.0, "");
    out.push_str(&format!(
        "<g transform=\"matrix(1 0 0 -1 0 {})\">",
        n(2.0 * SEA)
    ));
    for range in RANGES {
        out.push_str(&format!("<use href=\"#{}-land\"/>", range.name));
    }
    land_haze(out);
    out.push_str("</g></g>");
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
    streaks(out);
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

/// The sky's light along the ripples: a stroke along a row wherever the
/// water holds enough of it to show, and a heavier one where it holds
/// more. Rows of a like height are drawn together, as one path of one
/// width.
fn streaks(out: &mut String) {
    /// Half-rows to a band, and the least light that shows, faint and
    /// strong.
    const BAND: usize = 8;
    const LEAST: [f64; 2] = [0.03, 0.07];
    out.push_str(&format!(
        "<g fill=\"none\" stroke=\"{}\" stroke-linecap=\"round\">",
        SEA_SHEEN.hex()
    ));
    let lines: Vec<f64> = (0..)
        .map(|half| row(SEA) + 0.25 + 0.5 * f64::from(half))
        .take_while(|&line| line < row(HEIGHT))
        .collect();
    for band in lines.chunks(BAND) {
        let middle = band.iter().sum::<f64>() / band.len() as f64;
        let tall = height(middle + 0.5) - height(middle - 0.5);
        let mut strokes = [String::new(), String::new()];
        for &line in band {
            let y = height(line);
            let step = 2.0 * ripple((y - SEA) / (HEIGHT - SEA));
            for (start, end, mean) in runs((0.0, WIDTH), step, LEAST[0], |x| sheen(x, y)) {
                let strong = usize::from(mean >= LEAST[1]);
                strokes[strong].push_str(&format!("M{} {}H{}", n(start), n(y), n(end + step)));
            }
        }
        for (path, opacity) in strokes.iter().zip([0.05, 0.11]) {
            if !path.is_empty() {
                out.push_str(&format!(
                    "<path d=\"{path}\" stroke-width=\"{:.2}\" opacity=\"{opacity}\"/>",
                    0.45 * tall
                ));
            }
        }
    }
    out.push_str("</g>");
}

/// Points of the haze's light on the ripples, most under the lighthouse
/// and along a streak.
fn glitter(out: &mut String) {
    out.push_str(&format!("<g fill=\"{}\">", SEA_SHEEN.hex()));
    for fleck in 0..420 {
        let id = (fleck, 7);
        let x = AXIS - 760.0 + 1400.0 * hash(id, 201);
        let line = 10.0 + (56.0 * hash(id, 202)).floor();
        let (y, tall) = (height(line + 0.5), height(line + 1.0) - height(line));
        let depth = (y - SEA) / (HEIGHT - SEA);
        let aside = (x - AXIS) / 430.0;
        let lit = (-aside * aside).exp() * (0.3 + 0.7 * smoothstep(0.02, 0.3, depth));
        // None in the near shore's dark.
        let shaded = (-320.0..600.0).contains(&(x - AXIS)) && y < STRAND + 46.0;
        if shaded || hash(id, 203) > 0.12 + 0.88 * lit {
            continue;
        }
        let held = smoothstep(0.44, 0.84, ripples(place(x, y)));
        let size = hash(id, 204);
        let long = ripple(depth) * 3.0 * mix(0.05, 0.16, size);
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{:.2}\" rx=\"{:.2}\" opacity=\"{}\"/>",
            n(x - long),
            n(y - 0.15 * tall),
            n(2.0 * long),
            0.3 * tall,
            0.15 * tall,
            dim((0.05 + 0.55 * lit) * (0.4 + 1.1 * held) * mix(0.45, 1.0, size))
        ));
    }
    out.push_str("</g>");
}

/// The lamp's path on the water, from the rocks under the lighthouse to
/// the viewer: a column of bands of every length, close and bright along
/// its middle. It is drawn in three layers, each inside the last and
/// brighter: where the lamp shows at all, where it shows well, and where
/// it shows best.
fn lamp_path(out: &mut String) {
    /// How brightly the lamp must show for each layer, and how brightly
    /// that layer's stretches show it on the whole.
    const LAYERS: [(f64, f64); 3] = [(0.2, 0.3), (0.45, 0.56), (0.7, 0.86)];
    /// Lines to a row of ripples.
    const LINES: f64 = 3.0;
    let mut covered = 0.0;
    for (least, shown) in LAYERS {
        // With the scene's clock at nothing no beam faces the viewer.
        let over = 0.72 * shown;
        let mut path = String::new();
        let mut line = (row(STRAND) * LINES).ceil() / LINES;
        while line < row(HEIGHT) {
            let (top, foot) = (height(line), height(line + 1.0 / LINES));
            let y = (top + foot) / 2.0;
            for (start, end, _) in runs((AXIS - 130.0, AXIS + 130.0), 1.0, least, |x| {
                lamp_shows(x, y)
            }) {
                path.push_str(&format!(
                    "M{} {}h{}v{:.2}h-{}z",
                    n(start - 0.5),
                    n(top),
                    n(end - start + 1.0),
                    foot - top,
                    n(end - start + 1.0)
                ));
            }
            line += 1.0 / LINES;
        }
        // Each layer lies over the last, and is given only what the
        // layers under it have left to cover.
        out.push_str(&format!(
            "<path d=\"{path}\" fill=\"{}\" opacity=\"{}\"/>",
            GOLD_DEEP.mix(GOLD, shown).hex(),
            dim((over - covered) / (1.0 - covered))
        ));
        covered = over;
    }
}

#[cfg(test)]
mod tests;
