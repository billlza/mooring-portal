//! The near shore of the drawn image: the pier, the block the tower stands
//! on, and the lighthouse. Each part is the part of the same name in the
//! live scene's `shader-shore.mjs`, as it stands when the scene's clock
//! reads nothing. Places are measured from the lighthouse's axis, as
//! there.

use super::field::{hash, smoothstep};
use super::{
    AXIS, BALL, BALL_R, BAND, CAP, DECK, DECK_HALF, DECK_TOP, DOME, DOME_HALF, DOOR, EAVES,
    EAVES_HALF, FOOT, FOOT_HALF, FRAME, GLASS, GLASS_DIM, GLASS_FOOT, GLASS_HALF, GLASS_HOT,
    GLASS_TOP, IRON, IRON_LIT, JOINT, LAMP, LAMPLIGHT, NECK, NECK_HALF, PANE, PIER, PIER_FOOT,
    PIER_RISE, PIER_SINK, PIER_TOP, PLASTER, PLASTER_DARK, PLASTER_LIT, PLINTH_FOOT, PLINTH_LEFT,
    PLINTH_RIGHT, RAIL, RAIL_HALF, ROOM_HALF, Rgb, SPIKE, STONE, STONE_DARK, STONE_LIT, STRAND,
    WALL, WALL_DARK, WALL_WARM, WHITE, WIDTH, gradient, n, polygon, rect,
};

const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
/// Where the mooring posts stand on the pier.
const BOLLARDS: [f64; 2] = [95.0, 148.0];

/// How much of the low light from behind on the left reaches a round wall
/// at `u` across it (-1 its left edge, 1 its right).
fn rim(u: f64) -> f64 {
    1.0 - smoothstep(-0.8, -0.15, u)
}

fn plaster(u: f64) -> Rgb {
    PLASTER
        .mix(PLASTER_DARK, smoothstep(-0.2, 1.0, u))
        .mix(PLASTER_LIT, rim(u))
}

fn iron(u: f64) -> Rgb {
    IRON.mix(IRON_LIT, 0.9 * rim(u))
}

/// The pier's upper and lower edges at `x` from the axis.
pub(super) fn pier_top(x: f64) -> f64 {
    PIER_TOP - PIER_RISE * (x - PIER)
}

pub(super) fn pier_foot(x: f64) -> f64 {
    PIER_FOOT + PIER_SINK * (x - PIER)
}

/// Half the tower's width at height `y`: its shaft, the plinth band at its
/// foot, and the flare that carries the gallery.
fn girth(y: f64) -> f64 {
    if y >= NECK {
        let shaft = NECK_HALF + (FOOT_HALF - NECK_HALF) * (y - NECK) / (FOOT - NECK);
        return if y >= BAND { shaft + 2.0 } else { shaft };
    }
    let s = ((NECK - y) / (NECK - DECK)).clamp(0.0, 1.0);
    NECK_HALF + (DECK_HALF - 1.5 - NECK_HALF) * s * s
}

/// A gradient across a round thing `half` wide about the axis, `paint`
/// giving its colour at each place across it.
fn across(out: &mut String, id: &str, middle: f64, half: f64, paint: impl Fn(f64) -> Rgb) {
    let stops: Vec<(f64, Rgb, f64)> = [-1.0, -0.8, -0.64, -0.48, -0.32, -0.15, 0.4, 1.0]
        .into_iter()
        .map(|u| ((u + 1.0) / 2.0, paint(u), 1.0))
        .collect();
    gradient(
        out,
        &format!(
            "linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"0\"",
            n(AXIS + middle - half),
            n(AXIS + middle + half)
        ),
        &stops,
    );
}

/// A line of the given width; `more` is any further attributes.
fn line(out: &mut String, from: (f64, f64), to: (f64, f64), stroke: &str, width: f64, more: &str) {
    out.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"{width}\"{more}/>",
        n(from.0),
        n(from.1),
        n(to.0),
        n(to.1)
    ));
}

pub(super) fn draw(out: &mut String) {
    paints(out);
    pier(out);
    for x in BOLLARDS {
        bollard(out, x);
    }
    rect(
        out,
        (AXIS + PLINTH_LEFT, FOOT),
        (AXIS + PLINTH_RIGHT, PLINTH_FOOT),
        "url(#plinth)",
        "",
    );
    tower(out);
    head(out);
}

fn paints(out: &mut String) {
    out.push_str("<defs>");
    let end = WIDTH - AXIS;
    out.push_str(&format!(
        "<clipPath id=\"pier\"><polygon points=\"{},{} {},{} {},{} {},{}\"/></clipPath>",
        n(AXIS + PIER),
        n(pier_top(PIER)),
        n(WIDTH),
        n(pier_top(end)),
        n(WIDTH),
        n(pier_foot(end)),
        n(AXIS + PIER),
        n(pier_foot(PIER))
    ));
    gradient(
        out,
        &format!(
            "radialGradient id=\"warmth\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"200\"",
            n(AXIS + PIER),
            n(pier_top(PIER) + 8.0)
        ),
        &[
            (0.0, WALL_WARM, 0.8),
            (0.3, WALL_WARM, 0.6),
            (0.65, WALL_WARM, 0.2),
            (1.0, WALL_WARM, 0.0),
        ],
    );
    gradient(
        out,
        &format!(
            "linearGradient id=\"ledge\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"0\"",
            AXIS + 30.0,
            AXIS + 200.0
        ),
        &[(0.0, STONE_LIT, 0.55), (1.0, STONE_LIT, 0.22)],
    );
    gradient(
        out,
        &format!(
            "linearGradient id=\"wet\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"{}\" x2=\"0\" y2=\"{STRAND}\"",
            STRAND - 26.0
        ),
        &[(0.0, BLACK, 0.0), (1.0, BLACK, 0.5)],
    );
    for (index, x) in BOLLARDS.into_iter().enumerate() {
        across(out, &format!("post{index}"), x, 8.8, |u| {
            iron(u).scaled(0.7)
        });
        across(out, &format!("knob{index}"), x, 8.8, |u| {
            iron(u).scaled(1.2)
        });
    }
    gradient(
        out,
        "linearGradient id=\"plinth\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"",
        &[
            (0.0, STONE_LIT, 1.0),
            (0.08, STONE, 1.0),
            (0.5, STONE.mix(STONE_DARK, 0.5), 1.0),
            (0.93, STONE_DARK, 1.0),
        ],
    );
    gradient(
        out,
        "linearGradient id=\"pane\"",
        &[
            (0.0, PANE, 1.0),
            (0.5, PANE.mix(GLASS_HOT, 0.45), 1.0),
            (1.0, PANE, 1.0),
        ],
    );
    gradient(
        out,
        "linearGradient id=\"shadow\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"",
        &[(0.0, BLACK, 0.4), (1.0, BLACK, 0.08)],
    );
    across(out, "roof", 0.0, DOME_HALF, iron);
    across(out, "eaves", 0.0, EAVES_HALF, |u| iron(u).scaled(1.25));
    across(out, "room", 0.0, ROOM_HALF, |u| {
        IRON.mix(LAMPLIGHT, 0.85 * rim(u))
    });
    across(out, "deck", 0.0, DECK_HALF, |u| {
        IRON.scaled(1.5).mix(PLASTER_LIT, 0.8 * rim(u))
    });
    across(out, "glass", 0.0, GLASS_HALF, |u| {
        // The beam on the near side shows at the glass's right edge.
        let band = 0.35 * (-(u - 1.0) * (u - 1.0) / 0.09).exp();
        GLASS_DIM
            .mix(GLASS, (1.0 - 0.8 * u.abs() + 0.3 * band).clamp(0.0, 1.0))
            .mix(GLASS_HOT, (0.8 * band).clamp(0.0, 1.0))
    });
    gradient(
        out,
        "radialGradient id=\"glare\"",
        &[
            (0.0, GLASS_HOT, 0.85),
            (0.25, GLASS_HOT, 0.66),
            (0.5, GLASS_HOT, 0.31),
            (0.75, GLASS_HOT, 0.09),
            (1.0, GLASS_HOT, 0.0),
        ],
    );
    gradient(
        out,
        "radialGradient id=\"gleam\"",
        &[
            (0.0, IRON_LIT, 0.55),
            (0.5, IRON_LIT, 0.25),
            (1.0, IRON_LIT, 0.0),
        ],
    );
    out.push_str(&format!(
        "<clipPath id=\"pane-room\"><rect x=\"{}\" y=\"{GLASS_TOP}\" width=\"{}\" height=\"{}\"/></clipPath>",
        n(AXIS - GLASS_HALF),
        n(2.0 * GLASS_HALF),
        n(GLASS_FOOT - GLASS_TOP)
    ));
    out.push_str("</defs>");
}

/// The pier: its blocks, the warmth of the lamp at its near end, the ledge
/// under its cap, its joints, and the sky caught along its top edge.
fn pier(out: &mut String) {
    let end = WIDTH - AXIS;
    let mut block = 0;
    loop {
        let from = PIER + f64::from(block) * JOINT;
        if from >= end {
            break;
        }
        let to = (from + JOINT).min(end);
        let warmth = 1.0 - smoothstep(30.0, 200.0, (from + to) / 2.0);
        let tone = hash((block, 3), 71);
        let wall = WALL_DARK
            .mix(WALL, 0.25 + 0.75 * warmth)
            .scaled(0.86 + 0.2 * tone);
        // A hair wider than the block, so that no seam shows between two.
        polygon(
            out,
            &[
                (AXIS + from, pier_top(from)),
                (AXIS + to + 0.4, pier_top(to)),
                (AXIS + to + 0.4, pier_foot(to)),
                (AXIS + from, pier_foot(from)),
            ],
            &wall.hex(),
            "",
        );
        if block > 0 {
            line(
                out,
                (AXIS + from, pier_top(from) + CAP),
                (AXIS + from, pier_foot(from)),
                "#000000",
                1.8,
                " stroke-opacity=\"0.45\"",
            );
        }
        block += 1;
    }
    let whole = [
        (AXIS + PIER, pier_top(PIER)),
        (WIDTH, pier_top(end)),
        (WIDTH, pier_foot(end)),
        (AXIS + PIER, pier_foot(PIER)),
    ];
    polygon(out, &whole, "url(#warmth)", " clip-path=\"url(#pier)\"");
    polygon(out, &whole, "url(#wet)", "");
    line(
        out,
        (AXIS + PIER, pier_top(PIER) + CAP),
        (WIDTH, pier_top(end) + CAP),
        "url(#ledge)",
        2.6,
        "",
    );
    line(
        out,
        (AXIS + PIER, pier_top(PIER) + 1.0),
        (WIDTH, pier_top(end) + 1.0),
        &STONE_LIT.hex(),
        2.0,
        " stroke-opacity=\"0.8\"",
    );
}

/// A mooring post on the pier, standing at `x` from the axis.
fn bollard(out: &mut String, x: f64) {
    let index = usize::from(x > BOLLARDS[0]);
    let (at, base) = (AXIS + x, pier_top(x));
    let post = format!("url(#post{index})");
    rect(out, (at - 4.6, base - 17.0), (at + 4.6, base), &post, "");
    rect(out, (at - 6.6, base - 2.8), (at + 6.6, base), &post, "");
    out.push_str(&format!(
        "<ellipse cx=\"{}\" cy=\"{}\" rx=\"8.8\" ry=\"4.8\" fill=\"url(#knob{index})\"/>",
        n(at),
        n(base - 18.0)
    ));
}

/// The tower: its plaster in strips that follow its taper, the gallery's
/// shadow and brackets on its flare, its band, windows and door.
fn tower(out: &mut String) {
    // The heights at which its outline turns, each with half its width.
    let mut outline: Vec<(f64, f64)> = (0..=5)
        .map(|step| DECK + (NECK - DECK) * f64::from(step) / 5.0)
        .map(|y| (y, girth(y)))
        .collect();
    outline.push((BAND, girth(BAND) - 2.0));
    outline.push((BAND, girth(BAND)));
    outline.push((FOOT, girth(FOOT)));
    const EDGES: [f64; 20] = [
        -1.0, -0.92, -0.84, -0.76, -0.68, -0.6, -0.52, -0.44, -0.36, -0.28, -0.2, -0.1, 0.05, 0.2,
        0.35, 0.5, 0.65, 0.8, 0.9, 1.0,
    ];
    for pair in EDGES.windows(2) {
        if let [left, right] = *pair {
            // A hair past its right edge, so that no seam shows.
            let overlap = if right < 1.0 { 0.012 } else { 0.0 };
            let mut strip: Vec<(f64, f64)> = outline
                .iter()
                .map(|&(y, half)| (AXIS + left * half, y))
                .collect();
            strip.extend(
                outline
                    .iter()
                    .rev()
                    .map(|&(y, half)| (AXIS + (right + overlap) * half, y)),
            );
            polygon(out, &strip, &plaster((left + right) / 2.0).hex(), "");
        }
    }
    let flare: Vec<(f64, f64)> = outline.iter().take(6).copied().collect();
    let mut under: Vec<(f64, f64)> = flare.iter().map(|&(y, half)| (AXIS - half, y)).collect();
    under.extend(flare.iter().rev().map(|&(y, half)| (AXIS + half, y)));
    polygon(out, &under, "url(#shadow)", "");
    // The brackets that carry the gallery, set about the tower's round.
    for bracket in -3..=3 {
        let u = (0.45 * f64::from(bracket)).sin();
        let run: Vec<(f64, f64)> = flare
            .iter()
            .map(|&(y, half)| (AXIS + u * half, y))
            .collect();
        out.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"#000000\" stroke-opacity=\"0.22\" stroke-width=\"2.4\"/>",
            super::points(&run)
        ));
    }
    line(
        out,
        (AXIS - girth(BAND), BAND),
        (AXIS + girth(BAND), BAND),
        &WHITE.hex(),
        2.6,
        " stroke-opacity=\"0.1\"",
    );
    for (middle, half) in [((18.5, 354.5), (3.5, 8.5)), ((21.5, 428.0), (4.0, 9.0))] {
        let (x, y) = (AXIS + middle.0, middle.1);
        rect(
            out,
            (x - half.0 - 1.4, y - half.1 - 1.4),
            (x + half.0 + 1.4, y + half.1 + 1.4),
            &IRON.hex(),
            "",
        );
        rect(
            out,
            (x - half.0, y - half.1),
            (x + half.0, y + half.1),
            "url(#pane)",
            "",
        );
    }
    // The door, in its frame, under a small gable.
    let door = AXIS + 22.0;
    polygon(
        out,
        &[(door - 10.0, 479.0), (door, 469.0), (door + 10.0, 479.0)],
        &FRAME.hex(),
        "",
    );
    rect(
        out,
        (door - 9.0, 477.0),
        (door + 9.0, 515.0),
        &FRAME.hex(),
        "",
    );
    rect(
        out,
        (door - 7.0, 479.0),
        (door + 7.0, 513.0),
        &DOOR.hex(),
        "",
    );
}

/// The top of the lighthouse: spike, ball, dome, eaves, the lantern's
/// glass, the room under it, the gallery's deck and its railing.
fn head(out: &mut String) {
    rect(
        out,
        (AXIS - 0.7, SPIKE),
        (AXIS + 0.7, BALL),
        "url(#roof)",
        "",
    );
    rect(
        out,
        (AXIS - 2.3, BALL),
        (AXIS + 2.3, DOME + 2.0),
        "url(#roof)",
        "",
    );
    out.push_str(&format!(
        "<circle cx=\"{AXIS}\" cy=\"{BALL}\" r=\"{BALL_R}\" fill=\"url(#roof)\"/>\
         <path d=\"M{} {base} A{DOME_HALF} {} 0 0 1 {} {base} Z\" fill=\"url(#roof)\"/>\
         <ellipse cx=\"{}\" cy=\"{}\" rx=\"12.6\" ry=\"7.2\" fill=\"url(#gleam)\"/>",
        n(AXIS - DOME_HALF),
        n(EAVES + 1.0 - DOME),
        n(AXIS + DOME_HALF),
        n(AXIS - 9.0),
        n(DOME + 7.0),
        base = n(EAVES + 1.0),
    ));
    rect(
        out,
        (AXIS - EAVES_HALF, EAVES),
        (AXIS + EAVES_HALF, EAVES + 4.5),
        "url(#eaves)",
        "",
    );
    rect(
        out,
        (AXIS - GLASS_HALF, GLASS_TOP),
        (AXIS + GLASS_HALF, GLASS_FOOT),
        "url(#glass)",
        "",
    );
    // The lamp's glare, low in the middle of the glass.
    out.push_str(&format!(
        "<ellipse cx=\"{AXIS}\" cy=\"{LAMP}\" rx=\"16\" ry=\"20\" fill=\"url(#glare)\" clip-path=\"url(#pane-room)\"/>"
    ));
    let bar = IRON.scaled(1.6).hex();
    for x in [-17.8, -6.8, 6.8, 17.8] {
        rect(
            out,
            (AXIS + x - 0.55, GLASS_TOP),
            (AXIS + x + 0.55, GLASS_FOOT),
            &bar,
            " opacity=\"0.85\"",
        );
    }
    rect(
        out,
        (AXIS - GLASS_HALF, 256.55),
        (AXIS + GLASS_HALF, 257.45),
        &bar,
        " opacity=\"0.85\"",
    );
    rect(
        out,
        (AXIS - ROOM_HALF, GLASS_FOOT),
        (AXIS + ROOM_HALF, DECK_TOP),
        "url(#room)",
        "",
    );
    for panel in -2..=2 {
        let x = AXIS + ROOM_HALF * (0.52 * f64::from(panel)).sin();
        line(
            out,
            (x, GLASS_FOOT),
            (x, DECK_TOP),
            "#000000",
            1.0,
            " stroke-opacity=\"0.2\"",
        );
    }
    rect(
        out,
        (AXIS - DECK_HALF, DECK_TOP),
        (AXIS + DECK_HALF, DECK),
        "url(#deck)",
        "",
    );
    // The railing: its posts set about the gallery's round, and two rails.
    out.push_str("<g stroke-opacity=\"0.9\">");
    for post in -7..=6 {
        let u = ((f64::from(post) + 0.5) * 0.21).sin();
        let x = AXIS + RAIL_HALF * u;
        line(
            out,
            (x, RAIL - 0.7),
            (x, DECK_TOP),
            &iron(u).scaled(0.9).hex(),
            0.8,
            "",
        );
    }
    for (y, width) in [(RAIL, 1.4), ((RAIL + DECK_TOP) / 2.0, 0.8)] {
        line(
            out,
            (AXIS - RAIL_HALF, y),
            (AXIS + RAIL_HALF, y),
            &iron(0.0).scaled(0.9).hex(),
            width,
            "",
        );
    }
    out.push_str("</g>");
}
