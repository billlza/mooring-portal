//! The far shore's ranges: their crests against what the shader takes for
//! granted of them, the outlines and paints the image draws of them, and
//! the rules the drawing and the shader share for them.

use super::super::field::{climb, noise1, rough, woods};
use super::*;
use crate::assets::module;

/// The salts of the ranges' roughness.
const SALTS: [u32; 3] = [141, 121, 131];

#[test]
fn the_ranges_are_named_from_the_farthest_with_their_salts() {
    let names: Vec<&str> = RANGES.iter().map(|range| range.name).collect();
    assert_eq!(names, ["remote", "far", "near"]);
    assert_eq!(RANGES.map(|range| range.salt), SALTS);
}

#[test]
fn the_farther_a_range_stands_the_paler_its_land_and_the_lower_its_mist() {
    for pair in RANGES.windows(2) {
        let (farther, nearer) = (pair[0], pair[1]);
        let (Rgb(r, g, b), Rgb(near_r, near_g, near_b)) = (farther.land, nearer.land);
        assert!(
            r > near_r && g > near_g && b > near_b,
            "{} and {}",
            farther.name,
            nearer.name
        );
        assert!(farther.mist <= nearer.mist, "{}", farther.name);
    }
    assert!(RANGES[0].mist < RANGES[1].mist);
    // Yet the palest stands darker than the sky at the waterline behind
    // it, so that it shows as land and not as fog.
    let (Rgb(r, g, b), Rgb(sky_r, sky_g, sky_b)) = (RANGES[0].land, SKY_RIM);
    assert!(r < sky_r && g < sky_g && b < sky_b);
}

#[test]
fn noise_along_a_line_passes_through_its_lattice_and_stays_between() {
    for salt in SALTS {
        for corner in -3..40 {
            let (a, b) = (hash((corner, 0), salt), hash((corner + 1, 0), salt));
            let at = f64::from(corner);
            assert_eq!(noise1(at, salt), a);
            // Half-way between two lattice points it is their mean.
            assert!((noise1(at + 0.5, salt) - (a + b) / 2.0).abs() < 1e-12);
            for step in 1..10 {
                let value = noise1(at + f64::from(step) / 10.0, salt);
                assert!(value >= a.min(b) - 1e-12 && value <= a.max(b) + 1e-12);
            }
        }
    }
}

#[test]
fn a_crests_roughness_keeps_within_its_bounds() {
    let mut widest: (f64, f64) = (0.0, 0.0);
    for step in 0..=12_640 {
        let x = f64::from(step) * 0.25;
        for salt in SALTS {
            let (hills, edge) = (rough(x, salt), woods(x, salt));
            // Five sizes, each half as tall as the last from a half.
            assert!(hills.abs() <= 0.968_75, "{x} {salt}");
            assert!(edge.abs() <= 1.05, "{x} {salt}");
            widest = (widest.0.max(hills.abs()), widest.1.max(edge.abs()));
        }
    }
    // And neither is idle.
    assert!(widest.0 > 0.4 && widest.1 > 0.4, "{widest:?}");
}

#[test]
fn a_crest_climbs_no_higher_than_the_shader_looks_and_sinks_with_its_line() {
    let mut highest = f64::INFINITY;
    for range in RANGES {
        for step in 0..=6_320 {
            let x = f64::from(step) * 0.5;
            let (level, top) = (along(range.line, x), range.ridge(x));
            // The shader passes over places clear above this.
            assert!((top - level).abs() <= climb(level), "{} at {x}", range.name);
            // Where the line lies under the water, so does the crest, and
            // left of where the outline begins there is nothing above it.
            if level >= SEA + 4.0 {
                assert_eq!(top, level, "{} at {x}", range.name);
            }
            if x < range.from {
                assert!(top >= SEA, "{} at {x}", range.name);
            }
            highest = highest.min(top);
        }
    }
    // The shader looks for no crest above 420. Every crest stands lower by
    // more than half a pixel even where the scene draws coarsest: on a
    // screen 320 points wide the hero is 282 tall, and at one pixel to the
    // point and the watchdog's lowest quality a pixel spans 5.6 units.
    assert!(highest - 420.0 > 2.8, "{highest}");
    assert!(module("shader-sky.mjs").contains("\n  if (p.y < 420.0) {\n    return c;\n  }\n"));
}

#[test]
fn each_range_shows_where_the_picture_has_it() {
    // The farthest rises on the left, behind the town's coast, and the
    // far range's peak hides it; the far range's peak stands over the
    // town, and the nearer hill hides the far range beyond the
    // lighthouse; the nearer hill shows all along its length.
    for (index, shown, hidden) in [
        (0, 1600.0, 2265.0),
        (1, 2265.0, 2800.0),
        (2, 2800.0, 2200.0),
    ] {
        assert!(shows(index, shown), "{} at {shown}", RANGES[index].name);
        assert!(!shows(index, hidden), "{} at {hidden}", RANGES[index].name);
    }
    // Where a nearer range stands higher, the range behind it does not
    // show, and where the water covers it neither does it.
    for step in 0..=316 {
        let x = f64::from(step) * 10.0;
        for index in 0..RANGES.len() {
            let top = RANGES[index].ridge(x);
            let covered = top >= SEA || RANGES[index + 1..].iter().any(|near| near.ridge(x) <= top);
            assert_eq!(shows(index, x), !covered, "{} at {x}", RANGES[index].name);
        }
    }
}

#[test]
fn an_outline_follows_its_crest_closely_wherever_its_range_shows() {
    for (index, range) in RANGES.iter().enumerate() {
        let shape = outline(index);
        assert_eq!(shape[0], (range.from, SEA + 2.0));
        assert_eq!(shape[shape.len() - 1], (WIDTH, SEA + 2.0));
        let crest = &shape[1..shape.len() - 1];
        assert!(crest.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for &(x, y) in crest {
            assert_eq!(y, range.ridge(x), "{} at {x}", range.name);
        }
        for pair in crest.windows(2) {
            let gap = pair[1].0 - pair[0].0;
            assert!(gap <= COARSE + 1e-9, "{} after {}", range.name, pair[0].0);
            // A gap wider than the fine step passes over nothing that shows.
            let mut x = pair[0].0 + FINE;
            while x < pair[1].0 - 1e-9 {
                assert!(!shows(index, x), "{} shows at {x}", range.name);
                x += FINE;
            }
        }
        // Each range shows along a good stretch of the far shore, and is
        // hidden along another.
        let places: Vec<bool> = (0..1_000)
            .map(|step| {
                shows(
                    index,
                    range.from + f64::from(step) * (WIDTH - range.from) / 1_000.0,
                )
            })
            .collect();
        let shown = places.iter().filter(|&&shown| shown).count();
        assert!((150..1_000).contains(&shown), "{}: {shown}", range.name);
    }
}

/// The height at `x` of an outline's crest as the image draws it: straight
/// from point to point. `None` left of where the outline begins.
fn drawn(shape: &[(f64, f64)], x: f64) -> Option<f64> {
    let crest = &shape[1..shape.len() - 1];
    if x < crest[0].0 {
        return None;
    }
    let after = crest
        .partition_point(|point| point.0 < x)
        .clamp(1, crest.len() - 1);
    let ((x0, y0), (x1, y1)) = (crest[after - 1], crest[after]);
    Some(y0 + (y1 - y0) * (x - x0) / (x1 - x0))
}

#[test]
fn where_an_outline_cuts_across_its_crest_the_water_or_a_nearer_range_hides_it() {
    let shapes: Vec<Vec<(f64, f64)>> = (0..RANGES.len()).map(outline).collect();
    for (index, shape) in shapes.iter().enumerate() {
        let crest = &shape[1..shape.len() - 1];
        for pair in crest.windows(2) {
            let (from, to) = (pair[0].0, pair[1].0);
            if to - from <= FINE + 1e-9 {
                continue;
            }
            // Every half unit along the straight cut.
            let mut x = from + 0.5;
            while x < to {
                let y = drawn(shape, x).unwrap();
                let hidden = y >= SEA
                    || shapes[index + 1..]
                        .iter()
                        .any(|near| drawn(near, x).is_some_and(|top| top <= y));
                assert!(hidden, "{} shows at {x}, {y}", RANGES[index].name);
                x += 0.5;
            }
        }
    }
}

/// A colour as the picture writes it, in its three parts.
fn parts(hex: &str) -> [u8; 3] {
    [1, 3, 5].map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
}

#[test]
fn each_range_pales_into_the_mist_at_its_foot() {
    let image = super::super::draw();
    for range in RANGES {
        let opening = format!(
            "<linearGradient id=\"{}-paint\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"{}\" x2=\"0\" y2=\"547\">",
            range.name,
            n(SEA - range.mist)
        );
        let from = image.find(&opening).unwrap() + opening.len();
        let stops = &image[from..from + image[from..].find("</linearGradient>").unwrap()];
        let colours: Vec<&str> = stops
            .match_indices("stop-color=\"")
            .map(|(at, found)| &stops[at + found.len()..at + found.len() + 7])
            .collect();
        assert_eq!(colours.len(), 9, "{}", range.name);
        assert_eq!(colours[0], range.land.hex(), "{}", range.name);
        // A quarter of the way down it has paled as a smoothstep has
        // risen there, by 0.15625 of the way.
        assert_eq!(
            colours[2],
            range.land.mix(MIST, 0.7 * 0.156_25).hex(),
            "{}",
            range.name
        );
        assert_eq!(
            colours[8],
            range.land.mix(MIST, 0.7).hex(),
            "{}",
            range.name
        );
        // Every stop is at least as light as the one above it.
        for pair in colours.windows(2) {
            let (above, below) = (parts(pair[0]), parts(pair[1]));
            assert!(
                (0..3).all(|part| below[part] >= above[part]),
                "{}: {pair:?}",
                range.name
            );
        }
    }
    // The farthest range's mist reaches less high.
    assert!(image.contains(
        "<linearGradient id=\"remote-paint\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"510\""
    ));
}

#[test]
fn the_shader_and_the_drawing_share_the_ranges_rules() {
    // As `hero::tests` ties the rest of the picture: each line here stands
    // in the drawing's code and in the shader's; change one, and this
    // fails until the other follows.
    let close = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
    let shader = close(
        &["shader.mjs", "shader-sky.mjs", "shader-water.mjs"]
            .map(module)
            .concat(),
    );
    let drawing = close(&[include_str!("../field.rs"), include_str!("../sky.rs")].concat());
    for (drawn, painted) in [
        // Noise along a line.
        ("letf=x-x.floor();", "floatf=x-corner;"),
        (
            "mix(hash((corner,0),salt),hash((corner+1,0),salt),f*f*(3.0-2.0*f),)",
            "mix(hash(cell,salt),hash(cell+ivec2(1,0),salt),f*f*(3.0-2.0*f))",
        ),
        // A crest's hills and saddles in five sizes, each half as tall as
        // the last, and the ragged edge of its woods.
        (
            "letmutsum=0.0;letmuttall=0.5;letmutat=x/150.0;foriin0..5{",
            "floatsum=0.0;floattall=0.5;floatat=x/150.0;for(uinti=0u;i<5u;i++){",
        ),
        (
            "{sum+=tall*(2.0*noise1(at,salt+i)-1.0);tall*=0.5;at=at*2.03+0.37;}sum}",
            "{sum+=tall*(2.0*noise1(at,salt+i)-1.0);tall*=0.5;at=at*2.03+0.37;}returnsum;}",
        ),
        (
            "letedge=noise1(x/3.0,salt+5)-0.5+0.5*(noise1(x/1.3,salt+6)-0.5);edge*(0.4+noise1(x/70.0,salt+7))}",
            "floatedge=noise1(x/3.0,salt+5u)-0.5+0.5*(noise1(x/1.3,salt+6u)-0.5);returnedge*(0.4+noise1(x/70.0,salt+7u));}",
        ),
        // How tall a crest's hills and woods stand over its line, and the
        // most they may climb.
        (
            "lethigh=f64::max(SEA+4.0-level,0.0);letwooded=f64::min(high,8.0)/8.0;level-(0.16*high+1.2*wooded)*rough(x,salt)-1.1*wooded*woods(x,salt)}",
            "floathigh=max(SEA+4.0-level,0.0);floatwooded=min(high,8.0)/8.0;returnlevel-(0.16*high+1.2*wooded)*rough(x,salt)-1.1*wooded*woods(x,salt);}",
        ),
        (
            "lethigh=f64::max(SEA+4.0-level,0.0);0.16*high+2.4*f64::min(high,8.0)/8.0}",
            "floathigh=max(SEA+4.0-level,0.0);return0.16*high+2.4*min(high,8.0)/8.0;}",
        ),
        // A range's crest, from its own line and with its own salt.
        (
            "crest(along(self.line,x),x,self.salt)",
            "crest(along(r.from,r.count,x),x,r.salt)",
        ),
        // Each range's line, salt, land and how high its mist reaches.
        (
            "line:REMOTE,salt:141,land:REMOTE_LAND,mist:35.0,",
            "Range(REMOTE,REMOTE_POINTS,141u,REMOTE_LAND,0.2,35.0)",
        ),
        (
            "line:FAR,salt:121,land:FAR_LAND,mist:60.0,",
            "Range(FAR,FAR_POINTS,121u,FAR_LAND,0.6,60.0)",
        ),
        (
            "line:NEAR,salt:131,land:NEAR_LAND,mist:60.0,",
            "Range(NEAR,NEAR_POINTS,131u,NEAR_LAND,1.0,60.0)",
        ),
        // The mist at a range's foot, and the haze over its land.
        (
            "n(SEA-range.mist),n(MIST_FOOT)",
            "floatmist=smoothstep(SEA-r.mist,SEA+2.0,p.y);",
        ),
        ("constMIST_FOOT:f64=SEA+2.0;", "SEA+2.0,p.y);"),
        (
            "range.land.mix(MIST,LAND_MIST*smoothstep(0.0,1.0,t))",
            "land=mix(land,MIST,0.7*mist);",
        ),
        ("constLAND_MIST:f64=0.7;", "MIST,0.7*mist"),
        ("constLAND_HAZE:f64=0.4;", "mix(land,HAZE,0.4*veil(p))"),
    ] {
        assert!(drawing.contains(drawn), "the drawing: {drawn}");
        assert!(shader.contains(painted), "the shader: {painted}");
    }
    // The water mirrors the ranges' land but not the lie of their slopes,
    // and the scene works out the sky and the sea from one call each, so
    // that the ranges are compiled into it no more than twice.
    assert!(shader.contains("returnhills(sky(m,veil(m)),m,false);"));
    assert!(shader.contains("c=hills(c,p,true);"));
    assert_eq!(shader.matches("hills(").count(), 3);
    assert_eq!(shader.matches("heaven(p)").count(), 1);
    assert_eq!(shader.matches("sea(p,angle)").count(), 1);
}
