//! The rocks: how a shape is cut, and that the pile's boulders are what
//! the shader's are, each every place nearer its own point than another.

use super::*;

fn near(a: Place, b: Place) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

/// Every boulder that shows above the water, with its cell and its point:
/// the cells `draw` looks through.
fn pile() -> Vec<((i32, i32), Place)> {
    let mut found = Vec::new();
    for row in 25..=31 {
        for column in -13..=4 {
            let point = boulder_at((column, row));
            if piled(point) && boulder((column, row), point).len() >= 3 {
                found.push(((column, row), point));
            }
        }
    }
    found
}

#[test]
fn a_shape_is_cut_along_a_line() {
    let square = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    // What is left of the line x = 1 is kept: the normal points right.
    let left = cut(&square, (1.0, 5.0), (1.0, 0.0));
    let expected = [(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (0.0, 2.0)];
    assert_eq!(left.len(), expected.len());
    for (place, wanted) in left.iter().zip(expected) {
        assert!(near(*place, wanted), "{place:?}");
    }
    // A corner cut off leaves five.
    let corner = cut(&square, (2.0, 1.0), (1.0, 1.0));
    assert_eq!(corner.len(), 5);
    assert!(corner.iter().all(|place| place.0 + place.1 <= 3.0 + 1e-9));
    assert!(corner.iter().any(|place| near(*place, (2.0, 1.0))));
    assert!(corner.iter().any(|place| near(*place, (1.0, 2.0))));
    // A line that misses the shape keeps all of it, or none.
    assert_eq!(cut(&square, (5.0, 0.0), (1.0, 0.0)), square);
    assert!(cut(&square, (-5.0, 0.0), (1.0, 0.0)).is_empty());
    // A corner on the line is kept, once.
    assert_eq!(cut(&square, (2.0, 0.0), (1.0, 0.0)), square);
}

#[test]
fn a_boulder_is_every_place_nearest_its_own_point() {
    let pile = pile();
    assert!((30..60).contains(&pile.len()), "{} boulders", pile.len());
    for &(cell, point) in &pile {
        // Its point keeps clear of its cell's edges, and lies under the
        // pile's line, between the pile's ends.
        let within = |at: f64, cell: i32, size: f64| {
            let across = at / size - f64::from(cell);
            (0.22..0.78).contains(&across)
        };
        assert!(within(point.0, cell.0, BOULDER.0) && within(point.1, cell.1, BOULDER.1));
        assert!(point.1 >= along(ROCKS, point.0) + 5.0 && (-313.0..=77.0).contains(&point.0));
        let shape = boulder(cell, point);
        assert!(shape.len() >= 3, "{cell:?}");
        for &corner in &shape {
            assert!(
                corner.1 <= STRAND + 1.0 + 1e-9,
                "{cell:?} stands in the water"
            );
            let own = (corner.0 - point.0).hypot(corner.1 - point.1);
            // No other point of the lattice is nearer to any of its
            // corners, as far out as a corner can lie.
            for row in -3..=3 {
                for column in -3..=3 {
                    let other = boulder_at((cell.0 + column, cell.1 + row));
                    let far = (corner.0 - other.0).hypot(corner.1 - other.1);
                    assert!(
                        own <= far + 1e-6,
                        "{cell:?}: {corner:?} is nearer {other:?}"
                    );
                }
            }
        }
        // And the point is inside it, unless the water has cut it off.
        let inside = shape.iter().enumerate().all(|(index, &a)| {
            let b = shape[(index + 1) % shape.len()];
            (b.0 - a.0) * (point.1 - a.1) - (b.1 - a.1) * (point.0 - a.0) >= 0.0
        });
        assert_eq!(inside, point.1 <= STRAND + 1.0, "{cell:?}");
    }
    // Some of the pile stands in the water, and most of it out of it.
    let wet = pile.iter().filter(|(_, point)| point.1 > STRAND + 1.0);
    assert!((1..pile.len() / 3).contains(&wet.count()));
    // The pile is under its line: none stands above the tower's foot.
    assert!(pile.iter().all(|(_, point)| point.1 > 505.0));
    // A point above the line, or beyond its ends, has no boulder.
    assert!(!piled((-150.0, 500.0)) && piled((-150.0, 560.0)));
    assert!(!piled((-320.0, 596.0)) && !piled((80.0, 596.0)));
}

#[test]
fn a_face_is_brightest_where_it_turns_up_and_toward_the_light() {
    // The light comes from above on the left: a face looking that way is
    // lit, one looking down and to the right is not.
    let toward = (-0.67_f64).atan2(-0.74);
    assert!(face_light(toward) > 0.99);
    assert!(face_light(toward + std::f64::consts::PI) < 1e-9);
    let across = face_light(toward + std::f64::consts::FRAC_PI_2);
    assert!(across > 0.05 && across < 0.3, "{across}");
}

#[test]
fn the_pile_is_drawn_as_its_boulders_faces() {
    let mut out = String::new();
    draw(&mut out);
    let pile = pile();
    // One outline to a boulder, in the group drawn last, and at most
    // five faces before it.
    let (faces, seams) = out.split_once("<g stroke=").unwrap();
    assert_eq!(seams.matches("<polygon").count(), pile.len());
    let count = faces.matches("<polygon").count();
    assert!(count > 3 * pile.len() && count <= 5 * pile.len(), "{count}");
    assert!(seams.ends_with("</g>") && !faces.contains("<g"));
    // The brightest stone is on the lamp's side, the darkest at the water.
    assert!(seams.starts_with("\"#0d0e11\" stroke-opacity=\"0.75\""));
}
