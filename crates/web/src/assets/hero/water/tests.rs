//! The water: its measure, its arithmetic against values worked out
//! separately from the shader's own definitions, and what the image draws
//! of it.

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// The part of the image between `opening` and the next `closing`.
fn part<'a>(image: &'a str, opening: &str, closing: &str) -> &'a str {
    let from = image
        .find(opening)
        .unwrap_or_else(|| panic!("no {opening}"))
        + opening.len();
    &image[from..from + image[from..].find(closing).unwrap()]
}

/// The value of the attribute `name` in the tag `tag`.
fn attribute(tag: &str, name: &str) -> String {
    let opening = format!(" {name}=\"");
    part(tag, &opening, "\"").to_owned()
}

fn number(text: &str) -> f64 {
    text.parse()
        .unwrap_or_else(|_| panic!("not a number: {text}"))
}

/// The strokes of one of the image's paths of streaks: each with its
/// height and its two ends.
fn strokes(path: &str) -> Vec<(f64, f64, f64)> {
    attribute(path, "d")
        .split('M')
        .skip(1)
        .map(|stroke| {
            let (start, rest) = stroke.split_once(' ').unwrap();
            let (y, end) = rest.split_once('H').unwrap();
            (number(y), number(start), number(end))
        })
        .collect()
}

/// The stretches of one layer of the lamp's path: each with its top, its
/// two ends and its height.
fn stretches(layer: &str) -> Vec<(f64, f64, f64, f64)> {
    attribute(layer, "d")
        .split('M')
        .skip(1)
        .map(|stretch| {
            let (start, rest) = stretch.split_once(' ').unwrap();
            let (top, rest) = rest.split_once('h').unwrap();
            let (wide, rest) = rest.split_once('v').unwrap();
            let (tall, back) = rest.split_once("h-").unwrap();
            assert_eq!(back, format!("{wide}z"), "a stretch closes on itself");
            let start = number(start);
            (number(top), start, start + number(wide), number(tall))
        })
        .collect()
}

#[test]
fn rows_are_counted_from_the_far_shore_and_grow_toward_the_viewer() {
    assert!(close(row(SEA), 9.690_400_414_690_93));
    assert!(close(row(HEIGHT), 66.025_001_005_647_68));
    let mut last = 0.0;
    for line in 10..66 {
        let line = f64::from(line);
        assert!(close(row(height(line)), line), "{line}");
        // A row is as tall as a ripple is long where it lies.
        let tall = height(line + 1.0) - height(line);
        let long = ripple((height(line + 0.5) - SEA) / (HEIGHT - SEA));
        assert!(tall > last && (tall / long - 1.0).abs() < 0.02, "{line}");
        last = tall;
    }
    // A place is measured in ripples from the lighthouse's axis, and a
    // ripple is 1.4 long at the far shore and 9.9 at the picture's foot.
    let (across, rows) = place(AXIS + 14.0, SEA);
    assert!(close(across, 10.0) && close(rows, row(SEA)));
    assert!(close(place(AXIS - 99.0, HEIGHT).0, -10.0));
}

#[test]
fn the_ripples_and_their_sway_match_their_reference_values() {
    // What a second writing of the shader's `ripples` and `sway` gives,
    // in another language, with the scene's clock at nothing.
    for (at, face, slid) in [
        ((0.0, 10.0), 0.703_468_521_129, -0.213_831_899_412),
        ((37.5, 22.25), 0.450_477_382_501, 0.579_358_339_277),
        ((-212.0, 48.6), 0.372_613_051_477, -0.340_889_250_776),
        ((5.3, 60.1), 0.500_220_565_877, 0.587_562_455_529),
    ] {
        assert!(close(ripples(at), face), "{at:?}: {}", ripples(at));
        assert!(close(sway(at), slid), "{at:?}: {}", sway(at));
    }
}

#[test]
fn the_skys_light_and_the_lamp_show_where_the_reference_has_them() {
    // The same of the light the water holds, across the picture: under
    // the haze and far from it, by the far shore and near the viewer.
    for ((x, y), light) in [
        ((2300.0, 620.0), 0.046_138_224_608),
        ((2900.0, 560.0), 0.011_965_327_759),
        ((2576.0, 545.5), 0.000_528_750_840),
        ((944.0, 653.8), 0.021_104_000_289),
        ((1615.0, 601.9), 0.021_152_474_855),
        ((2225.0, 671.1), 0.026_701_752_992),
        ((2225.0, 723.0), 0.022_256_511_329),
        ((2957.0, 567.3), 0.036_238_158_682),
        ((1859.0, 584.6), 0.047_358_069_488),
        ((2576.0, 700.0), 0.0),
    ] {
        assert!(close(sheen(x, y), light), "({x}, {y}): {}", sheen(x, y));
    }
    // And of the lamp's path: along its middle, toward its edges, at the
    // rocks' foot where it has not begun, and beside it.
    for ((x, y), shown) in [
        ((2570.0, 618.5), 0.286_321_116_626),
        ((2582.0, 618.5), 0.330_601_102_009),
        ((2573.0, 622.2), 0.677_940_304_477),
        ((2582.0, 622.2), 0.517_620_929_176),
        ((2570.0, 625.9), 0.783_582_648_751),
        ((2567.0, 640.7), 0.705_054_405_616),
        ((2576.0, 640.0), 1.0),
        ((2576.0, 598.0), 0.0),
        ((2700.0, 700.0), 0.0),
    ] {
        assert!(
            close(lamp_shows(x, y), shown),
            "({x}, {y}): {}",
            lamp_shows(x, y)
        );
    }
}

#[test]
fn runs_gives_each_stretch_its_ends_and_its_mean() {
    let values = [0.0, 1.0, 1.0, 0.0, 0.0, 2.0, 4.0, 0.0, 3.0, 3.0, 3.0];
    let at = |x: f64| values[x as usize];
    // A stretch begins and ends on a place that reaches the least, and
    // one still open at the line's end is closed there.
    assert_eq!(
        runs((0.0, 10.0), 1.0, 1.0, at),
        vec![(1.0, 2.0, 1.0), (5.0, 6.0, 3.0), (8.0, 10.0, 3.0)]
    );
    assert_eq!(
        runs((2.0, 8.0), 2.0, 1.0, at),
        vec![(2.0, 2.0, 1.0), (6.0, 8.0, 3.5)]
    );
    assert_eq!(runs((0.0, 7.0), 1.0, 2.0, at), vec![(5.0, 6.0, 3.0)]);
    assert!(runs((0.0, 10.0), 1.0, 5.0, at).is_empty());
}

/// The heights of the lines the image draws its streaks along. The image
/// writes a height to a tenth of a unit; the light is asked for at the
/// height itself.
fn streak_lines() -> Vec<f64> {
    (0..)
        .map(|half| row(SEA) + 0.25 + 0.5 * f64::from(half))
        .take_while(|&line| line < row(HEIGHT))
        .map(height)
        .collect()
}

/// The one of `lines` nearest `y`.
fn nearest(lines: &[f64], y: f64) -> f64 {
    let found = lines
        .iter()
        .copied()
        .min_by(|a, b| (a - y).abs().total_cmp(&(b - y).abs()))
        .unwrap();
    assert!((found - y).abs() < 0.051, "{y} is on no line");
    found
}

/// Every stroke of the image's streaks: whether it is a strong one, the
/// height of its line, and the first and last of the places along the
/// line, counted in steps, that it covers.
fn streaks_drawn(image: &str) -> Vec<(bool, f64, f64, f64)> {
    let lines = streak_lines();
    let group = part(image, "stroke-linecap=\"round\">", "</g>");
    let mut found = Vec::new();
    for path in group.split("<path").skip(1) {
        let opacity = attribute(path, "opacity");
        assert!(["0.05", "0.11"].contains(&opacity.as_str()), "{opacity}");
        for (y, start, end) in strokes(path) {
            let y = nearest(&lines, y);
            let step = 2.0 * ripple((y - SEA) / (HEIGHT - SEA));
            let (first, last) = ((start / step).round(), (end / step).round() - 1.0);
            assert!(
                (first * step - start).abs() < 0.051 && first <= last,
                "({start}, {y})"
            );
            found.push((opacity == "0.11", y, first, last));
        }
    }
    found
}

#[test]
fn the_streaks_lie_where_the_water_holds_the_skys_light() {
    let image = super::super::draw();
    let group = part(
        &image,
        &format!(
            "<g fill=\"none\" stroke=\"{}\" stroke-linecap=\"round\">",
            SEA_SHEEN.hex()
        ),
        "</g>",
    );
    let paths: Vec<&str> = group.split("<path").skip(1).collect();
    assert!((13..=30).contains(&paths.len()), "{} paths", paths.len());
    // Rows of a like height share a path, faint or strong, and the rows
    // grow toward the viewer: about six times from first to last.
    let widths: Vec<f64> = paths
        .iter()
        .map(|path| number(&attribute(path, "stroke-width")))
        .collect();
    assert!(widths.is_sorted(), "{widths:?}");
    let (first, last) = (widths[0], widths[widths.len() - 1]);
    assert!(
        (0.6..0.8).contains(&first) && (3.8..4.6).contains(&last),
        "{widths:?}"
    );
    let (mut faint, mut strong) = (0.0, 0.0);
    let (mut under, mut beside) = (0.0, 0.0);
    for (is_strong, y, first, last) in streaks_drawn(&image) {
        let step = 2.0 * ripple((y - SEA) / (HEIGHT - SEA));
        let light = |place: f64| sheen(place * step, y);
        // A stroke begins where the light begins and ends where it ends:
        // the places before and after it hold too little to show.
        assert!(
            light(first) >= 0.03 && light(last) >= 0.03,
            "({first}, {y})"
        );
        assert!(first == 0.0 || light(first - 1.0) < 0.03, "({first}, {y})");
        assert!(
            (last + 1.0) * step > WIDTH || light(last + 1.0) < 0.03,
            "({last}, {y})"
        );
        // A strong stroke holds more light along it than a faint one.
        let places = last - first + 1.0;
        let mean = (0..places as usize)
            .map(|place| light(first + place as f64))
            .sum::<f64>()
            / places;
        assert_eq!(is_strong, mean >= 0.07, "({first}, {y}): {mean}");
        let long = places * step;
        *(if is_strong { &mut strong } else { &mut faint }) += long;
        let middle = (first + last + 1.0) / 2.0 * step;
        if (middle - AXIS).abs() < 430.0 {
            under += long;
        } else if (middle - (AXIS - 1570.0)).abs() < 430.0 {
            beside += long;
        }
    }
    // Most strokes are faint, and there is far more light under the haze
    // around the lighthouse than as wide a stretch far to its left.
    assert!(
        strong > 0.0 && faint > 2.0 * strong,
        "{faint} faint, {strong} strong"
    );
    assert!(
        under > 3.0 * beside,
        "{under} under the haze, {beside} beside it"
    );
}

#[test]
fn every_place_that_holds_enough_light_is_under_a_stroke() {
    // The other way about: along a line, a place the image leaves bare
    // holds too little light to show.
    let image = super::super::draw();
    let drawn = streaks_drawn(&image);
    let lines = streak_lines();
    assert_eq!(lines.len(), 113);
    for &y in lines.iter().step_by(7) {
        let step = 2.0 * ripple((y - SEA) / (HEIGHT - SEA));
        let mut place = 0.0;
        while place * step <= WIDTH {
            let covered = drawn
                .iter()
                .any(|&(_, line, first, last)| line == y && first <= place && place <= last);
            assert_eq!(covered, sheen(place * step, y) >= 0.03, "({place}, {y})");
            place += 1.0;
        }
    }
}

#[test]
fn the_glitter_is_points_on_the_open_water() {
    let image = super::super::draw();
    let group = part(&image, &format!("<g fill=\"{}\">", SEA_SHEEN.hex()), "</g>");
    let points: Vec<&str> = group.split("<rect").skip(1).collect();
    assert!(points.len() > 80, "{} points", points.len());
    let mut bright = 0;
    for point in &points {
        let [x, y, wide, tall] =
            ["x", "y", "width", "height"].map(|name| number(&attribute(point, name)));
        // None is longer than a ripple, each is round at its ends, and
        // none lies in the near shore's dark.
        let long = ripple((y - SEA) / (HEIGHT - SEA));
        assert!(wide < long && tall < 0.35 * long, "{point}");
        assert!(
            (number(&attribute(point, "rx")) * 2.0 - tall).abs() < 0.011,
            "{point}"
        );
        assert!(SEA < y && y + tall < HEIGHT, "{point}");
        let shaded = (-319.0..599.0).contains(&(x + wide / 2.0 - AXIS)) && y < STRAND + 40.0;
        assert!(!shaded, "{point}");
        bright += usize::from(number(&attribute(point, "opacity")) > 0.3);
    }
    // A few are far brighter than the rest.
    assert!(bright > 3 && bright < points.len() / 4, "{bright} bright");
}

#[test]
fn the_lamps_path_is_three_layers_each_inside_the_last() {
    let image = super::super::draw();
    let layers: Vec<&str> = image
        .split('<')
        .filter(|tag| tag.starts_with("path d=\"M") && tag.contains("z\" fill=\"#"))
        .collect();
    assert_eq!(layers.len(), 3);
    // Each layer is brighter than the last, and is laid over it with
    // only what the last has left to cover: 0.72 of how brightly the
    // layer shows the lamp, in all.
    for (layer, (shown, opacity)) in
        layers
            .iter()
            .zip([(0.3, "0.22"), (0.56, "0.24"), (0.86, "0.36")])
    {
        assert_eq!(attribute(layer, "fill"), GOLD_DEEP.mix(GOLD, shown).hex());
        assert_eq!(attribute(layer, "opacity"), opacity);
    }
    let layers = [0, 1, 2].map(|layer| stretches(layers[layer]));
    // Three lines to a row of ripples, from the rocks' foot down. The
    // image writes a line's top to a tenth of a unit; the lamp is asked
    // for at the line's own middle.
    let lines: Vec<(f64, f64)> = (0..)
        .map(|third| (row(STRAND) * 3.0).ceil() / 3.0 + f64::from(third) / 3.0)
        .take_while(|&line| line < row(HEIGHT))
        .map(|line| (height(line), height(line + 1.0 / 3.0)))
        .collect();
    let tops: Vec<f64> = lines.iter().map(|line| line.0).collect();
    for (layer, least) in layers.iter().zip([0.2, 0.45, 0.7]) {
        assert!(layer.len() > 20, "{} stretches", layer.len());
        for &(top, start, end, tall) in layer {
            // The path runs from the rocks' foot to the viewer, under
            // the lighthouse, and each stretch begins and ends where the
            // lamp shows as brightly as its layer asks.
            let top = nearest(&tops, top);
            let foot = lines.iter().find(|line| line.0 == top).unwrap().1;
            assert!(top >= STRAND && (foot - top - tall).abs() < 0.006, "{top}");
            assert!(
                AXIS - 130.5 <= start && end <= AXIS + 130.5,
                "{start} {end}"
            );
            let y = (top + foot) / 2.0;
            assert!(lamp_shows(start + 0.5, y) >= least, "({start}, {y})");
            assert!(lamp_shows(end - 0.5, y) >= least, "({end}, {y})");
            assert!(lamp_shows(start - 0.5, y) < least, "({start}, {y})");
            assert!(lamp_shows(end + 0.5, y) < least, "({end}, {y})");
        }
    }
    // The other way about: within the path's reach, a place the image
    // leaves bare shows the lamp too faintly for its layer.
    for (layer, least) in layers.iter().zip([0.2, 0.45, 0.7]) {
        for &(top, foot) in &lines {
            let y = (top + foot) / 2.0;
            for step in 0..=260 {
                let x = AXIS - 130.0 + f64::from(step);
                let covered = layer.iter().any(|&(written, start, end, _)| {
                    (written - top).abs() < 0.051 && start < x && x < end
                });
                assert_eq!(covered, lamp_shows(x, y) >= least, "({x}, {y})");
            }
        }
    }
    // A stretch of a brighter layer lies within one of the layer under
    // it, on the same line.
    for pair in layers.windows(2) {
        for &(top, start, end, _) in &pair[1] {
            assert!(
                pair[0]
                    .iter()
                    .any(|&(under, from, to, _)| close(under, top) && from <= start && end <= to),
                "({start}..{end}, {top})"
            );
        }
    }
    // The path is close and bright along its middle: its brightest
    // stretches lie nearer the axis than its faintest reach.
    let reach = |layer: &[(f64, f64, f64, f64)]| {
        layer
            .iter()
            .map(|&(_, start, end, _)| (start - AXIS).abs().max((end - AXIS).abs()))
            .fold(0.0, f64::max)
    };
    assert!(reach(&layers[2]) < reach(&layers[0]));
    // It widens as it comes: its stretches near the viewer reach further
    // from the axis than those by the rocks.
    let (by_rocks, by_viewer): (Vec<_>, Vec<_>) = layers[0]
        .iter()
        .copied()
        .partition(|&(top, ..)| top < STRAND + 60.0);
    assert!(reach(&by_viewer) > reach(&by_rocks) + 5.0);
}

#[test]
fn the_shader_and_the_drawing_share_the_waters_rules() {
    // Each line here stands in the drawing's water and in the shader's;
    // change one, and this fails until the other follows.
    let close = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
    let shader = close(crate::assets::module("shader-water.mjs"));
    let drawing = close(include_str!("../water.rs"));
    for rule in [
        // The water's rows, its ripples and the light they hold.
        "1.4",
        "8.5*depth",
        "28.8*",
        "/17.0+",
        "*1.5+bend",
        "*3.3+5.0-1.7*bend",
        "across*0.12)",
        "across*0.35)",
        "across*0.16",
        "smoothstep(0.44,0.84,face)",
        "smoothstep(0.25,0.8,noise(",
        "/380.0",
        "(0.25+0.75*open)",
        "(0.3+0.7*glow)",
        "mix(0.2,0.09,depth)",
        "/430.0",
        // The glitter.
        "(0.05+0.55*lit)",
        "(0.4+1.1*held)",
        "mix(0.05,0.16,size)",
        "mix(0.45,1.0,size)",
        // The lamp's path.
        "mix(0.3,0.92,smoothstep(0.0,1.3,off))",
        "*2.4+3.0+bend",
        "*1.1+11.0-bend",
        "smoothstep(need,need+0.3,tilt)*strength",
        "9.0+0.17*under",
        "smoothstep(2.0,26.0,under)*(1.0-0.6*smoothstep(80.0,200.0,under))",
    ] {
        assert!(drawing.contains(rule), "the drawing: {rule}");
        assert!(shader.contains(rule), "the shader: {rule}");
    }
    for (drawn, painted) in [
        // The ripples' two sizes, their bend, and what a row slides.
        (
            "0.45*(at.0/17.0+at.1*0.8).sin()",
            "0.45*wave(at.x/17.0+at.y*0.8)",
        ),
        (
            "0.62*noise(at.0/22.0,",
            "0.62*noise(vec2(at.x/22.0+u_time*0.03,",
        ),
        (
            "0.38*noise(at.0/8.0,",
            "0.38*noise(vec2(at.x/8.0-u_time*0.07,",
        ),
        ("0.6*(row*1.9+", "0.6*wave(row*1.9+"),
        ("0.3*(row*4.3+", "0.3*wave(row*4.3-"),
        (
            "0.5*(noise(across*0.16,row*0.6)-0.5)",
            "0.5*(noise(vec2(across*0.16+t*0.05,row*0.6-t*0.2))-0.5)",
        ),
        // The lamp's path: what tilts a ripple toward it, how far the
        // row has slid it, and how much of the water it covers.
        (
            "0.4*(at.0/6.0+at.1*1.3).sin()",
            "0.4*wave(at.x/6.0+at.y*1.3)",
        ),
        (
            "0.6*noise(at.0/4.5,",
            "0.6*noise(vec2(at.x/4.5+u_time*0.11,",
        ),
        (
            "0.4*noise(at.0/9.0,",
            "0.4*noise(vec2(at.x/9.0-u_time*0.06,",
        ),
        ("sway(at)*ripple*0.6", "sway*ripple*0.6)"),
        ("0.72*shown", "path*(0.72+0.28*facing*facing)"),
    ] {
        assert!(drawing.contains(drawn), "the drawing: {drawn}");
        assert!(shader.contains(painted), "the shader: {painted}");
    }
}
