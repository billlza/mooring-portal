//! The live scene's arithmetic (`assets/js/shader.mjs`), written again in
//! Rust, so that the drawn image puts every star, lamp, crest and boulder
//! where the scene's shader does. Each function here is the function of
//! the same name there, and reads the same numbers.

use super::{LINE, SEA};

/// PCG (O'Neill 2014), the RXS-M-XS output for one 32-bit word.
pub(super) const fn pcg(v: u32) -> u32 {
    let state = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// A number in [0, 1) for a lattice cell; the salt picks which.
pub(super) fn hash(cell: (i32, i32), salt: u32) -> f64 {
    // As the shader turns a signed cell into an unsigned word.
    let x = cell.0.wrapping_add(32_768).cast_unsigned();
    let y = cell.1.wrapping_add(32_768).cast_unsigned();
    let mixed = pcg(x.wrapping_add(pcg(y.wrapping_add(salt.wrapping_mul(2_654_435_761)))));
    f64::from(mixed >> 8) / 16_777_216.0
}

/// The lattice cell a place falls in.
pub(super) fn cell(x: f64, y: f64) -> (i32, i32) {
    // Every place in the picture is within a few thousand units of its
    // corner, far inside what an `i32` holds.
    (x.floor() as i32, y.floor() as i32)
}

pub(super) fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub(super) fn smoothstep(from: f64, to: f64, x: f64) -> f64 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth noise in [0, 1].
pub(super) fn noise(x: f64, y: f64) -> f64 {
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (ex, ey) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (cx, cy) = cell(x, y);
    let a = hash((cx, cy), 11);
    let b = hash((cx + 1, cy), 11);
    let c = hash((cx, cy + 1), 11);
    let d = hash((cx + 1, cy + 1), 11);
    mix(mix(a, b, ex), mix(c, d, ex), ey)
}

/// Noise in four sizes, each half the last: about [0, 1].
pub(super) fn cloudy(x: f64, y: f64) -> f64 {
    (noise(x, y)
        + 0.5 * noise(x * 2.03 + 17.0, y * 2.03 + 17.0)
        + 0.25 * noise(x * 4.01 + 31.0, y * 4.01 + 31.0)
        + 0.125 * noise(x * 8.05 + 47.0, y * 8.05 + 47.0))
        / 1.875
}

/// The height at `x` of the line `LINE[from .. from + count]`: straight
/// from point to point, eased a little at each. Beyond its ends it is
/// level.
pub(super) fn along((from, count): (usize, usize), x: f64) -> f64 {
    let points = &LINE[from..from + count];
    let mut y = points.first().map_or(0.0, |point| point.1);
    for pair in points.windows(2) {
        if let [a, b] = *pair
            && x >= a.0
        {
            let t = ((x - a.0) / (b.0 - a.0)).min(1.0);
            y = mix(a.1, b.1, mix(t, t * t * (3.0 - 2.0 * t), 0.5));
        }
    }
    y
}

/// Smooth noise along a line, in [0, 1]: [`noise`] in one dimension.
pub(super) fn noise1(x: f64, salt: u32) -> f64 {
    let f = x - x.floor();
    let (corner, _) = cell(x, 0.0);
    mix(
        hash((corner, 0), salt),
        hash((corner + 1, 0), salt),
        f * f * (3.0 - 2.0 * f),
    )
}

/// The hills and saddles along a crest at `x`, within [-1, 1]: noise in
/// five sizes from 150 picture units down, each half as tall as the last.
pub(super) fn rough(x: f64, salt: u32) -> f64 {
    let mut sum = 0.0;
    let mut tall = 0.5;
    let mut at = x / 150.0;
    for i in 0..5 {
        sum += tall * (2.0 * noise1(at, salt + i) - 1.0);
        tall *= 0.5;
        at = at * 2.03 + 0.37;
    }
    sum
}

/// The ragged edge of the woods along a crest at `x`, within
/// [-1.05, 1.05]: thicker along some stretches than others.
pub(super) fn woods(x: f64, salt: u32) -> f64 {
    let edge = noise1(x / 3.0, salt + 5) - 0.5 + 0.5 * (noise1(x / 1.3, salt + 6) - 0.5);
    edge * (0.4 + noise1(x / 70.0, salt + 7))
}

/// A range's crest at `x`, where its line is at `level`: hills and
/// saddles as tall as the range stands above the water, and its woods'
/// edge.
pub(super) fn crest(level: f64, x: f64, salt: u32) -> f64 {
    let high = f64::max(SEA + 4.0 - level, 0.0);
    let wooded = f64::min(high, 8.0) / 8.0;
    level - (0.16 * high + 1.2 * wooded) * rough(x, salt) - 1.1 * wooded * woods(x, salt)
}

/// The most a crest may climb above its line where the line is at
/// `level`. The shader uses it to pass over places clear above the
/// ranges; the drawing has no need of it, and keeps it to test that it
/// holds.
#[cfg(test)]
pub(super) fn climb(level: f64) -> f64 {
    let high = f64::max(SEA + 4.0 - level, 0.0);
    0.16 * high + 2.4 * f64::min(high, 8.0) / 8.0
}
