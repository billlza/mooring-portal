//! The rocks of the drawn image, piled against the plinth and the pier's
//! end, as the live scene's `rocks` cuts them: the pile into boulders, one
//! about each point of a broken lattice, and each boulder into five faces
//! about its point, a face lit by how far it turns up and toward the
//! light. Places are measured from the lighthouse's axis, as there.

use std::f64::consts::TAU;

use super::field::{along, hash, mix, smoothstep};
use super::{AXIS, ROCK, ROCK_DARK, ROCK_LIT, ROCKS, STRAND, polygon};

/// The lattice's cell.
const BOULDER: (f64, f64) = (27.0, 19.0);
/// The faces of a boulder.
pub(super) const FACES: i32 = 5;

type Place = (f64, f64);

/// Where the boulder of a lattice cell has its point. The point keeps
/// clear of its cell's edges.
fn boulder_at(cell: (i32, i32)) -> Place {
    (
        (f64::from(cell.0) + 0.22 + 0.56 * hash(cell, 81)) * BOULDER.0,
        (f64::from(cell.1) + 0.22 + 0.56 * hash(cell, 82)) * BOULDER.1,
    )
}

/// Whether the boulder with its point at `point` is part of the pile.
fn piled(point: Place) -> bool {
    (-313.0..=77.0).contains(&point.0) && point.1 >= along(ROCKS, point.0) + 5.0
}

/// How bright the face of a boulder is that looks along the angle `a`.
fn face_light(a: f64) -> f64 {
    smoothstep(-0.35, 0.95, -0.74 * a.cos() - 0.67 * a.sin())
}

/// The part of the convex `shape` on the near side of the line through
/// `on` that `normal` points away from.
fn cut(shape: &[Place], on: Place, normal: Place) -> Vec<Place> {
    let side = |place: Place| (place.0 - on.0) * normal.0 + (place.1 - on.1) * normal.1;
    let mut kept = Vec::with_capacity(shape.len() + 1);
    for (index, &here) in shape.iter().enumerate() {
        let next = shape[(index + 1) % shape.len()];
        let (a, b) = (side(here), side(next));
        if a <= 0.0 {
            kept.push(here);
        }
        if (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0) {
            let t = a / (a - b);
            kept.push((mix(here.0, next.0, t), mix(here.1, next.1, t)));
        }
    }
    kept
}

/// The boulder of `cell`: every place nearer its point than any other
/// point, down to the waterline.
fn boulder(cell: (i32, i32), point: Place) -> Vec<Place> {
    let (wide, tall) = (2.0 * BOULDER.0, 2.0 * BOULDER.1);
    let mut shape = vec![
        (point.0 - wide, point.1 - tall),
        (point.0 + wide, point.1 - tall),
        (point.0 + wide, point.1 + tall),
        (point.0 - wide, point.1 + tall),
    ];
    for row in -2..=2 {
        for column in -2..=2 {
            if (column, row) != (0, 0) {
                let other = boulder_at((cell.0 + column, cell.1 + row));
                let between = ((point.0 + other.0) / 2.0, (point.1 + other.1) / 2.0);
                shape = cut(&shape, between, (other.0 - point.0, other.1 - point.1));
            }
        }
    }
    cut(&shape, (0.0, STRAND + 1.0), (0.0, 1.0))
}

pub(super) fn draw(out: &mut String) {
    let sector = TAU / f64::from(FACES);
    let mut seams = String::new();
    for row in (480.0 / BOULDER.1) as i32..=((STRAND + 4.0) / BOULDER.1) as i32 {
        for column in (-345.0 / BOULDER.0).floor() as i32..=(110.0 / BOULDER.0) as i32 {
            let cell = (column, row);
            let point = boulder_at(cell);
            if !piled(point) {
                continue;
            }
            let shape = boulder(cell, point);
            let turned = TAU * hash(cell, 84);
            let tone = hash(cell, 83);
            // The lamp's side of the pile is the brighter.
            let warmth = if point.0 < 0.0 {
                mix(0.35, 1.0, smoothstep(-300.0, -40.0, point.0))
            } else {
                0.55
            };
            for face in 0..FACES {
                let (from, to) = (
                    f64::from(face) * sector - turned,
                    f64::from(face + 1) * sector - turned,
                );
                let part = cut(
                    &cut(&shape, point, (from.sin(), -from.cos())),
                    point,
                    (-to.sin(), to.cos()),
                );
                if part.len() < 3 {
                    continue;
                }
                let lit = face_light((f64::from(face) + 0.5) * sector - turned);
                let height = part.iter().map(|place| place.1).sum::<f64>() / part.len() as f64;
                let stone = ROCK_DARK
                    .mix(ROCK, 0.25 + 0.6 * tone)
                    .mix(ROCK_LIT, lit * warmth * (0.55 + 0.45 * tone))
                    .scaled(1.0 - 0.55 * smoothstep(STRAND - 18.0, STRAND, height));
                polygon(out, &from_axis(&part), &stone.hex(), "");
            }
            if shape.len() >= 3 {
                polygon(&mut seams, &from_axis(&shape), "none", "");
            }
        }
    }
    // The dark between one boulder and the next.
    out.push_str(&format!(
        "<g stroke=\"{}\" stroke-opacity=\"0.75\" stroke-width=\"1.6\" stroke-linejoin=\"round\">{seams}</g>",
        ROCK_DARK.scaled(0.6).hex()
    ));
}

/// A shape measured from the axis, as it lies in the picture.
fn from_axis(shape: &[Place]) -> Vec<Place> {
    shape.iter().map(|&(x, y)| (AXIS + x, y)).collect()
}

#[cfg(test)]
mod tests;
