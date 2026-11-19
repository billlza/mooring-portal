//! The hero image: what it is made of, and that it and the live scene's
//! shader are two writings of one picture.

use super::field::{along, cell, cloudy, hash, noise, pcg, smoothstep};
use super::*;
use crate::assets::module;

/// What a picture served under this site's policy must not hold: it
/// fetches nothing and runs nothing.
const FORBIDDEN: [&str; 6] = [
    "<script",
    "<style",
    "<image",
    "style=",
    "xlink",
    "<foreignObject",
];

/// The live scene's shader: every module that holds a part of it.
fn shader() -> String {
    [
        "shader.mjs",
        "shader-sky.mjs",
        "shader-shore.mjs",
        "shader-water.mjs",
    ]
    .map(module)
    .concat()
}

/// Every `url(#…)` and `href="#…"` in `document`.
fn references(document: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for (opening, closing) in [("url(#", ')'), ("href=\"#", '"')] {
        for (at, _) in document.match_indices(opening) {
            let rest = &document[at + opening.len()..];
            found.push(&rest[..rest.find(closing).unwrap()]);
        }
    }
    found
}

#[test]
fn the_image_is_deterministic_and_self_contained() {
    let image = draw();
    assert_eq!(image, draw());
    assert!(image.starts_with(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\">",
        VIEW.0, VIEW.1, VIEW.0, VIEW.1
    )));
    assert!(image.ends_with("</svg>"));
    // Nothing is fetched from elsewhere, and there is nothing to run.
    assert_eq!(image.matches("http").count(), 1, "only the namespace");
    for forbidden in FORBIDDEN {
        assert!(!image.contains(forbidden), "{forbidden}");
    }
    // It is one picture of a few thousand shapes, not a file to wait for.
    assert!(image.len() < 150_000, "{} bytes", image.len());
    assert!(!image.contains("NaN") && !image.contains("inf"));
}

/// What is wrong with `document` as XML, as far as its tags go: a tag that
/// is not closed, or closed out of turn, and an attribute written twice.
/// The drawing writes no text and no comments, so its tags are all of it.
fn faults(document: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for tag in document.split('<').skip(1) {
        let Some((tag, text)) = tag.split_once('>') else {
            found.push(format!("<{tag} is not closed"));
            continue;
        };
        if !text.is_empty() {
            found.push(format!("text after <{tag}>"));
        }
        if let Some(name) = tag.strip_prefix('/') {
            if open.pop() != Some(name) {
                found.push(format!("</{name}> closes nothing"));
            }
            continue;
        }
        let (tag, alone) = match tag.strip_suffix('/') {
            Some(tag) => (tag, true),
            None => (tag, false),
        };
        // With the quoted values gone, what is left is the names.
        let names: String = tag.split('"').step_by(2).collect();
        let mut names = names.split_whitespace();
        let name = names.next().unwrap_or_default();
        let mut seen = Vec::new();
        for attribute in names {
            if seen.contains(&attribute) {
                found.push(format!("<{name}> has {attribute} twice"));
            }
            if !attribute.ends_with('=') {
                found.push(format!("<{name}> has {attribute} without a value"));
            }
            seen.push(attribute);
        }
        if tag.matches('"').count() % 2 != 0 {
            found.push(format!("<{name}> has an open quotation"));
        }
        if !alone {
            open.push(&tag[..name.len()]);
        }
    }
    found.extend(open.iter().map(|name| format!("<{name}> is left open")));
    found
}

#[test]
fn the_image_is_well_formed() {
    assert_eq!(faults(&draw()), Vec::<String>::new());
    // The check itself.
    for (document, fault) in [
        ("<svg><g></svg>", "</svg> closes nothing"),
        ("<svg><rect x=\"1\" x=\"2\"/></svg>", "<rect> has x= twice"),
        ("<svg><rect x/></svg>", "<rect> has x without a value"),
        ("<svg><rect x=\"1/></svg>", "<rect> has an open quotation"),
        ("<svg>", "<svg> is left open"),
        ("<svg>words</svg>", "text after <svg>"),
        ("<svg></svg><g", "<g is not closed"),
    ] {
        assert!(
            faults(document).contains(&fault.to_owned()),
            "{document}: {:?}",
            faults(document)
        );
    }
    assert!(faults("<svg a=\"x y=1\" b=\"2\"><g/><g c=\"3\"></g></svg>").is_empty());
}

#[test]
fn the_image_holds_every_part_of_the_picture_in_its_order() {
    let image = draw();
    let count = |part: &str| image.matches(part).count();
    // The sky and its haze, the hills, the sea and what it mirrors.
    for (part, times) in [
        ("fill=\"url(#sky)\"", 1),
        // The sky's haze and the water's, each over the sky and over the
        // land of the far shore.
        ("fill=\"url(#haze)\"", 4),
        ("clip-path=\"url(#land)\"", 2),
        // The three ranges, each drawn once and turned over in the water.
        ("fill=\"url(#remote-paint)\" id=\"remote-land\"", 1),
        ("fill=\"url(#far-paint)\" id=\"far-land\"", 1),
        ("fill=\"url(#near-paint)\" id=\"near-land\"", 1),
        ("<use href=\"#remote-land\"/>", 2),
        ("<use href=\"#far-land\"/>", 2),
        ("<use href=\"#near-land\"/>", 2),
        ("transform=\"matrix(1 0 0 -1 0 1090)\"", 1),
        ("fill=\"url(#sea)\"", 1),
        ("fill=\"url(#lee)\"", 1),
        // The pier, its two posts, and the block the tower stands on.
        ("clip-path=\"url(#pier)\"", 1),
        ("rx=\"8.8\" ry=\"4.8\"", 2),
        ("fill=\"url(#plinth)\"", 1),
        // The tower's two windows, and the top of the lighthouse.
        ("fill=\"url(#pane)\"", 2),
        ("fill=\"url(#roof)\"", 4),
        ("fill=\"url(#eaves)\"", 1),
        ("fill=\"url(#glass)\"", 1),
        ("fill=\"url(#glare)\"", 1),
        ("fill=\"url(#room)\"", 1),
        ("fill=\"url(#deck)\"", 1),
        // The lamp's glow and its two beams, in three layers each.
        ("fill=\"url(#glow)\"", 1),
        ("fill=\"url(#beam-", 6),
    ] {
        assert_eq!(count(part), times, "{part}");
    }
    // The pier's blocks have a joint between each two, and the gallery
    // its fourteen posts and the room's five panels.
    let blocks = ((WIDTH - AXIS - PIER) / JOINT).ceil() as usize;
    assert_eq!(count("stroke-opacity=\"0.45\""), blocks - 1);
    assert_eq!(
        count("stroke-width=\"0.8\"/>"),
        15,
        "the posts and the lower rail"
    );
    assert_eq!(count("stroke-opacity=\"0.2\""), 5);
    // The tower's plaster is lit along its left edge, and its door is dark.
    assert!(count(&format!("fill=\"{}\"", PLASTER_LIT.hex())) >= 2);
    assert_eq!(count(&format!("fill=\"{}\"", DOOR.hex())), 1);
    // The stars by their hundreds, and the town's lamps twice each with
    // their paths on the water.
    let stars = count("<circle") - 2 * sky::lamps().len() - 2;
    assert!(stars > 400, "{stars} stars");
    assert_eq!(count("r=\"1.3\""), sky::lamps().len());
    assert_eq!(count("rx=\"1.2\" ry=\"11\""), sky::lamps().len());
    // The sky's light along the water's ripples, its glitter, and the
    // lamp's path in three layers (`water::tests` looks into each).
    assert_eq!(count("stroke-linecap=\"round\""), 1);
    assert_eq!(count(&format!("<g fill=\"{}\">", SEA_SHEEN.hex())), 1);
    assert_eq!(count("z\" fill=\"#"), 3);
    // Back to front: the sky, the sea, the shore, the rocks, the light.
    let order = [
        "fill=\"url(#sky)\"",
        "fill=\"url(#remote-paint)\" id=",
        "fill=\"url(#far-paint)\" id=",
        "fill=\"url(#near-paint)\" id=",
        "fill=\"url(#sea)\"",
        "<g transform=",
        "stroke-linecap=\"round\"",
        "fill=\"url(#lee)\"",
        "z\" fill=\"#",
        "clip-path=\"url(#pier)\"/>",
        "fill=\"url(#plinth)\"",
        "fill=\"url(#glass)\"",
        "<g stroke=\"#0d0e11\"",
        "fill=\"url(#glow)\"",
    ]
    .map(|part| image.find(part).unwrap_or_else(|| panic!("no {part}")));
    assert!(order.is_sorted(), "{order:?}");
}

#[test]
fn the_towns_lamps_stand_along_the_far_shore() {
    let lamps = sky::lamps();
    assert!((25..70).contains(&lamps.len()), "{} lamps", lamps.len());
    for lamp in &lamps {
        assert!((1930.0..2370.0).contains(&lamp.x), "{}", lamp.x);
        assert!(lamp.y <= SEA - 2.5 && lamp.y > SEA - 17.5, "{}", lamp.y);
        assert!(lamp.bright > 0.25 && lamp.bright <= 1.0, "{}", lamp.bright);
    }
    assert!(lamps.windows(2).all(|pair| pair[0].x < pair[1].x));
    // The town is far thicker with them than the coast before it.
    let town = lamps.iter().filter(|lamp| lamp.x >= 2170.0).count();
    assert!(town > 3 * (lamps.len() - town), "{town} of {}", lamps.len());
}

#[test]
fn the_image_defines_what_it_refers_to_once() {
    let image = draw();
    let references = references(&image);
    assert!(references.len() > 20);
    for id in references {
        assert_eq!(
            image.matches(&format!(" id=\"{id}\"")).count(),
            1,
            "#{id} is not defined once"
        );
    }
}

#[test]
fn the_shader_takes_its_measures_from_the_drawing() {
    let shader = shader();
    for (name, value) in MEASURES {
        assert!(
            shader.contains(&format!("\nconst float {name} = {value:?};\n")),
            "the shader has no {name} of {value:?}"
        );
    }
    // And has no measure the drawing does not know of: `TAU` apart.
    assert_eq!(
        shader.matches("\nconst float ").count(),
        MEASURES.len() + 1,
        "a measure of the shader's is not in MEASURES"
    );
    assert!(shader.contains("\nconst float TAU = 6.28318530718;\n"));
    // A boulder has as many faces in the shader as in the drawing.
    assert!(shader.contains(&format!("float sector = TAU / {}.0;", rocks::FACES)));
    // The lines through the far range, the nearer hill, the rocks and the
    // farthest range.
    let from = shader.find("const vec2 LINE[58] = vec2[58](").unwrap();
    let written: String = shader[from..shader[from..].find(");").unwrap() + from]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let points: Vec<String> = LINE
        .iter()
        .map(|(x, y)| format!("vec2({x:?},{y:?})"))
        .collect();
    assert_eq!(
        written,
        format!("constvec2LINE[58]=vec2[58]({}", points.join(","))
    );
    for (name, (from, count)) in [
        ("FAR", FAR),
        ("NEAR", NEAR),
        ("ROCKS", ROCKS),
        ("REMOTE", REMOTE),
    ] {
        assert!(shader.contains(&format!("\nconst int {name} = {from};\n")));
        assert!(shader.contains(&format!("\nconst int {name}_POINTS = {count};\n")));
    }
    assert_eq!(FAR.1 + NEAR.1 + ROCKS.1 + REMOTE.1, LINE.len());
    assert_eq!(
        (NEAR.0, ROCKS.0, REMOTE.0),
        (FAR.1, FAR.1 + NEAR.1, FAR.1 + NEAR.1 + ROCKS.1)
    );
}

#[test]
fn the_shader_paints_in_the_drawings_colours() {
    let shader = shader();
    for (name, Rgb(r, g, b)) in PALETTE {
        assert!(
            shader.contains(&format!(
                "\nconst vec3 {name} = rgb({r:?}, {g:?}, {b:?});\n"
            )),
            "the shader has no {name} of {r} {g} {b}"
        );
        for part in [r, g, b] {
            assert!(
                part.fract() == 0.0 && (0.0..=255.0).contains(&part),
                "{name}"
            );
        }
    }
    // Every colour the shader names is one of these: it writes `rgb(` once
    // for each, and once to say what `rgb` is.
    assert_eq!(shader.matches("\nconst vec3 ").count(), PALETTE.len());
    assert_eq!(shader.matches("rgb(").count(), PALETTE.len() + 1);
    assert!(shader.contains("\n#define rgb(r, g, b) (vec3(r, g, b) / 255.0)\n"));
}

#[test]
fn the_shader_and_the_drawing_share_their_rules() {
    // What is a rule and not a figure is written twice as well: where a
    // star or a lamp or a boulder goes, and how a round wall is lit. Each
    // line here stands in the drawing's code and in the shader's; change
    // one, and this fails until the other follows. The water's rules are
    // in `water::tests`, and the far shore's ranges' in `sky::tests`.
    let shader = shader();
    let drawing = [
        include_str!("field.rs"),
        include_str!("sky.rs"),
        include_str!("shore.rs"),
        include_str!("rocks.rs"),
        include_str!("light.rs"),
    ]
    .concat();
    let close = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
    let (shader, drawing) = (close(&shader), close(&drawing));
    // The hash's numbers, which the drawing writes with their digits
    // grouped.
    let ungrouped = drawing.replace('_', "");
    for number in [
        "747796405",
        "2891336453",
        "277803737",
        "2654435761",
        "16777216.0",
    ] {
        assert!(ungrouped.contains(number), "the drawing: {number}");
        assert!(shader.contains(number), "the shader: {number}");
    }
    for rule in [
        "/1.875",
        // The haze, the cloud and the stars.
        "0.95*",
        "smoothstep(215.0,300.0,",
        "smoothstep(420.0,500.0,",
        "radius=0.3+1.0*size*size*size;",
        "tint<0.14",
        "tint<0.24",
        // The town.
        "smoothstep(2170.0,2200.0,x)",
        "smoothstep(1930.0,1960.0,x)",
        "0.8*town+0.16*coast",
        // A round wall, the tower's girth, and the pier.
        "smoothstep(-0.8,-0.15,u)",
        "smoothstep(-0.2,1.0,u)",
        "(DECK_HALF-1.5-NECK_HALF)*s*s",
        "PIER_TOP-PIER_RISE*(x-PIER)",
        "PIER_FOOT+PIER_SINK*(x-PIER)",
        // The boulders and their faces.
        "+0.22+0.56*",
        "smoothstep(-0.35,0.95,",
        "smoothstep(-300.0,-40.0,",
        // A line's easing between its points.
        "mix(t,t*t*(3.0-2.0*t),0.5)",
    ] {
        assert!(drawing.contains(rule), "the drawing: {rule}");
        assert!(shader.contains(rule), "the shader: {rule}");
    }
    // The salts that pick a star's, a lamp's and a boulder's numbers.
    for (drawn, painted) in [
        ("hash(cell,1)>0.7", "hash(cell,1u)>0.7"),
        ("hash(cell,9)<0.08", "hash(cell,9u)<0.08"),
        ("hash(cell,42)>settled(x)", "hash(cell,42u)>settled(x)"),
        ("hash(cell,81)", "hash(cell,81u)"),
        ("hash(cell,84)", "hash(id,84u)"),
        (
            "along(ROCKS,point.0)+5.0",
            "along(ROCKS,ROCKS_POINTS,point.x)+5.0",
        ),
        ("TAU/f64::from(FACES)", "floatsector=TAU/5.0;"),
        // The lattices of the stars, the lamps and the boulders.
        ("constCELL:f64=46.0;", "constfloatCELL=46.0;"),
        ("constCELL:f64=6.0;", "constfloatCELL=6.0;"),
        (
            "constBOULDER:(f64,f64)=(27.0,19.0);",
            "constvec2BOULDER=vec2(27.0,19.0);",
        ),
        // The haze's middle and reach.
        (
            "constHAZE_LENGTHS:(f64,f64)=(383.0,180.0);",
            "vec2(AXIS,500.0))/vec2(383.0,180.0)",
        ),
        ("constHAZE_HEIGHT:f64=500.0;", "vec2(AXIS,500.0)"),
        // The mooring posts, the windows and the door.
        (
            "constBOLLARDS:[f64;2]=[95.0,148.0];",
            "c=bollard(c,q,95.0);c=bollard(c,q,148.0);",
        ),
        (
            "[((18.5,354.5),(3.5,8.5)),((21.5,428.0),(4.0,9.0))]",
            "vec2(18.5,354.5),vec2(3.5,8.5));c=window(c,q,vec2(21.5,428.0),vec2(4.0,9.0));",
        ),
        (
            "letdoor=AXIS+22.0;",
            "abs(q-vec2(22.0,496.0))-vec2(7.0,17.0)",
        ),
    ] {
        assert!(drawing.contains(drawn), "the drawing: {drawn}");
        assert!(shader.contains(painted), "the shader: {painted}");
    }
}

#[test]
fn the_shader_asks_for_whole_integers_and_takes_no_power() {
    // Its hash needs 32-bit integers, which a fragment shader must ask for,
    // and it takes no power of a number that may be negative.
    let shader = shader();
    assert!(
        shader.contains("precision highp float;\n") && shader.contains("precision highp int;\n")
    );
    assert!(!shader.contains("pow("));
}

#[test]
fn pcg_and_the_hash_match_their_reference_values() {
    // The values a second writing of PCG's RXS-M-XS output for 32 bits
    // gives, in another language and with whole numbers of any size cut
    // to 32 bits, so that a slip in the wrapping here would show.
    for (word, mixed) in [
        (0, 129_708_002),
        (1, 2_831_084_092),
        (2, 2_055_130_248),
        (0xffff_ffff, 3_861_530_882),
        (0xdead_beef, 1_730_779_506),
    ] {
        assert_eq!(pcg(word), mixed, "{word:#x}");
    }
    // And of the hash: a count of 2^-24ths, for cells in the picture, left
    // of it and above it.
    for (cell, salt, count) in [
        ((0, 0), 1, 9_372_573),
        ((12, 34), 5, 2_879_667),
        ((-1, -7), 1, 11_100_705),
        ((-40_000, 3), 1, 1_452_372),
        ((68, 11), 9, 8_381_156),
        ((429, 5), 42, 10_230_807),
    ] {
        assert_eq!(
            hash(cell, salt),
            f64::from(count) / 16_777_216.0,
            "{cell:?}"
        );
    }
}

#[test]
fn noise_is_smooth_and_passes_through_its_lattice() {
    // As the second writing has it, between lattice points.
    for ((x, y), value) in [
        ((3.25, 7.75), 0.745_671_565_411_612_4),
        ((-2.5, 0.125), 0.779_929_996_002_465_5),
        ((61.9, 3.5), 0.351_969_897_270_202_7),
    ] {
        assert!((noise(x, y) - value).abs() < 1e-12, "{x} {y}");
    }
    // At a lattice point it is that point's own number.
    assert_eq!(noise(3.0, 7.0), hash((3, 7), 11));
    assert_eq!(noise(-2.0, 0.0), hash((-2, 0), 11));
    assert_eq!(cell(-0.5, 2.9), (-1, 2));
    // Between two, it lies between them, and half-way it is their mean.
    let (a, b) = (hash((3, 7), 11), hash((4, 7), 11));
    assert!((noise(3.5, 7.0) - (a + b) / 2.0).abs() < 1e-12);
    for step in 0..=20 {
        let value = noise(3.0 + f64::from(step) / 20.0, 7.0);
        assert!(value >= a.min(b) - 1e-12 && value <= a.max(b) + 1e-12);
    }
    for step in 0..200 {
        let (x, y) = (f64::from(step) * 0.37 - 20.0, f64::from(step) * 0.11);
        assert!((0.0..=1.0).contains(&noise(x, y)));
        assert!((0.0..=1.0).contains(&cloudy(x, y)));
    }
    assert_eq!(smoothstep(2.0, 4.0, 1.0), 0.0);
    assert_eq!(smoothstep(2.0, 4.0, 3.0), 0.5);
    assert_eq!(smoothstep(2.0, 4.0, 9.0), 1.0);
    // It is eased at both ends: a quarter of the way along, it has come
    // less than a sixth of the way.
    assert_eq!(smoothstep(0.0, 4.0, 1.0), 0.156_25);
    assert_eq!(smoothstep(0.0, 4.0, 3.0), 0.843_75);
}

#[test]
fn a_line_passes_through_its_points_and_is_level_beyond_them() {
    for (name, line) in [
        ("far", FAR),
        ("near", NEAR),
        ("rocks", ROCKS),
        ("remote", REMOTE),
    ] {
        let points = &LINE[line.0..line.0 + line.1];
        for &(x, y) in points {
            assert!((along(line, x) - y).abs() < 1e-9, "{name} at {x}");
        }
        // Its points run from left to right.
        assert!(
            points.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "{name}"
        );
        let (first, last) = (points[0], points[points.len() - 1]);
        assert_eq!(along(line, first.0 - 500.0), first.1, "{name}");
        assert_eq!(along(line, last.0 + 500.0), last.1, "{name}");
        // A quarter of the way from one point to the next it has come
        // a little less than a quarter of the way: it is eased by half.
        let (a, b) = (points[1], points[2]);
        let quarter = along(line, a.0 + (b.0 - a.0) / 4.0);
        assert!(
            (quarter - (a.1 + (b.1 - a.1) * 0.203_125)).abs() < 1e-9,
            "{name}"
        );
        // Between two points it stays between their heights.
        for pair in points.windows(2) {
            let middle = along(line, (pair[0].0 + pair[1].0) / 2.0);
            let (low, high) = (pair[0].1.min(pair[1].1), pair[0].1.max(pair[1].1));
            assert!(middle >= low && middle <= high, "{name}");
        }
    }
}

#[test]
fn colours_are_written_as_six_digits() {
    assert_eq!(Rgb(5.0, 19.0, 32.0).hex(), "#051320");
    assert_eq!(Rgb(254.6, 0.4, 300.0).hex(), "#ff00ff");
    assert_eq!(Rgb(-4.0, 127.5, 128.0).hex(), "#008080");
    assert_eq!(
        Rgb(0.0, 100.0, 200.0).mix(Rgb(100.0, 0.0, 200.0), 0.25),
        Rgb(25.0, 75.0, 200.0)
    );
    assert_eq!(Rgb(10.0, 20.0, 30.0).scaled(1.5), Rgb(15.0, 30.0, 45.0));
    assert_eq!(
        (n(12.0), n(12.04), n(-0.26), n(3.95)),
        ("12".into(), "12".into(), "-0.3".into(), "4".into())
    );
    assert_eq!(
        (dim(0.126), dim(-1.0), dim(7.0)),
        ("0.13".into(), "0.00".into(), "1.00".into())
    );
}
