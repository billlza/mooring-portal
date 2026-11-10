//! The hero image: what it is made of, and that its layers are parts of
//! it.

use super::*;

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
    let first = draw();
    assert_eq!(first, draw());
    for picture in [&first.image, &first.land, &first.ring] {
        assert!(picture.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(picture.ends_with("</svg>"));
        // Nothing is fetched from elsewhere, and there is nothing to run.
        assert_eq!(picture.matches("http").count(), 1, "only the namespace");
        for forbidden in FORBIDDEN {
            assert!(!picture.contains(forbidden), "{forbidden}");
        }
        // A browser draws an SVG into a canvas only at a stated size.
        assert!(picture[..picture.find('>').unwrap()].contains(" width=\""));
        assert!(picture[..picture.find('>').unwrap()].contains(" height=\""));
    }
    assert!(first.image.len() < 80_000, "{} bytes", first.image.len());
}

#[test]
fn every_picture_defines_what_it_refers_to() {
    let hero = draw();
    for (name, picture) in [
        ("image", &hero.image),
        ("land", &hero.land),
        ("ring", &hero.ring),
    ] {
        let references = references(picture);
        assert!(!references.is_empty(), "{name}");
        for id in references {
            assert!(
                picture.contains(&format!(" id=\"{id}\"")),
                "{name} uses #{id} without defining it"
            );
        }
    }
}

#[test]
fn the_layers_are_parts_of_the_image_and_nothing_else() {
    let hero = draw();
    let mut rng = Rng(SEED);
    let ring = ring(&mut rng);
    let ridges = ridges(&mut rng);
    // The same shapes, to the byte, as the image draws.
    assert!(hero.image.contains(&ring) && hero.image.contains(&ridges));
    assert!(hero.ring.contains(&ring) && hero.land.contains(&ridges));
    let mut moorings = String::new();
    self::moorings(&mut moorings);
    figure(&mut moorings);
    assert!(hero.land.ends_with(&format!("{moorings}</svg>")));
    // The land stands on nothing: the scene paints the sky behind it,
    // the water under it and the lantern's light over it.
    for absent in [
        "url(#sky)",
        "url(#milky)",
        "url(#water)",
        "url(#lantern)",
        "id=\"ring\"",
    ] {
        assert!(!hero.land.contains(absent), "land has {absent}");
    }
    // Its ridges stop at the waterline, as the image's water hides them.
    assert!(hero.land.contains(&format!(
        "<clipPath id=\"above\"><rect width=\"{WIDTH}\" height=\"{SHORE}\"/></clipPath>"
    )));
    assert!(
        hero.land
            .contains("<g clip-path=\"url(#above)\"><use href=\"#ridges\"/></g>")
    );
    // The ring is alone in its square.
    for absent in ["url(#sky)", "url(#water)", "url(#lantern)", "id=\"ridges\""] {
        assert!(!hero.ring.contains(absent), "ring has {absent}");
    }
    assert!(hero.ring.ends_with("</defs><use href=\"#ring\"/></svg>"));
}

#[test]
fn the_geometry_is_where_the_image_draws() {
    let Geometry {
        width,
        height,
        pole_x,
        pole_y,
        span,
        shore,
        lantern_x,
        lantern_y,
    } = GEOMETRY;
    let hero = draw();
    assert!(hero.image.contains(&format!(
        "viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\""
    )));
    assert!(hero.land.contains(&format!(
        "viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\""
    )));
    // The pole is the ring's centre, and the ring layer is the square
    // of side `span` about it, wide enough for the ring's halo and glow.
    assert!(hero.image.contains(&format!(
        "<circle cx=\"{pole_x}\" cy=\"{pole_y}\" r=\"{RING_R}\" fill=\"none\" stroke=\"#ffffff\""
    )));
    assert!(hero.ring.contains(&format!(
        "viewBox=\"{} {} {span} {span}\" width=\"{span}\" height=\"{span}\"",
        pole_x - span / 2.0,
        pole_y - span / 2.0
    )));
    assert!(span / 2.0 >= RING_R + HALO + 3.0 * GLOW_BLUR);
    // The waterline is where the image's water begins.
    assert!(hero.image.contains(&format!(
        "<rect x=\"0\" y=\"{shore}\" width=\"{width}\" height=\"{}\" fill=\"url(#water)\"/>",
        height - shore
    )));
    // The lantern is the bright point at the masthead.
    assert!(hero.image.contains(&format!(
        "<circle cx=\"{lantern_x}\" cy=\"{lantern_y}\" r=\"2.4\" fill=\"#fff1c8\"/>"
    )));
}

#[test]
fn splitmix_matches_its_reference_sequence() {
    // SplitMix64 seeded with 0, as published with the algorithm.
    let mut rng = Rng(0);
    assert_eq!(rng.next(), 0xe220_a839_7b1d_cdaf);
    assert_eq!(rng.next(), 0x6e78_9e6a_a1b9_65f4);
    let unit = Rng(SEED).unit();
    assert!((0.0..1.0).contains(&unit));
}

#[test]
fn ridges_pass_through_their_peaks() {
    let mut rng = Rng(SEED);
    let peaks = [(0.0, 700.0), (800.0, 600.0), (1600.0, 700.0)];
    let line = ridge(&mut rng, &peaks, 20.0);
    assert_eq!(line.len(), 2 * 32 + 1);
    for peak in peaks {
        assert!(line.contains(&peak));
    }
    assert!(line.windows(2).all(|pair| pair[0].0 < pair[1].0));
}
