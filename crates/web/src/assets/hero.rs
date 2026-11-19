//! The home page's hero image, drawn by code: a night sky, a far shore
//! with a town's lamps at the water, and a lighthouse on its rocks at the
//! end of a pier, its lamp lit, over a dark sea that carries the lamp's
//! path.
//!
//! The drawing is deterministic, so the image, and with it its
//! content-hashed address, changes only when this code does. It uses
//! presentation attributes only, no `<style>` or script, so the content
//! security policy has nothing to allow.
//!
//! The home page's live scene (`assets/js/`) is the same picture in
//! motion, worked out by a shader with no image behind it, and this image
//! is that scene as it stands when its clock reads nothing: it is what the
//! page shows first, and all it shows without script. Where things stand
//! and what colour they are is therefore written twice, here and in the
//! shader, under the same names: the measures and the colours below, and
//! the lines of [`LINE`]. A test fails when one of the two is changed
//! without the other. The shader's arithmetic is written again in
//! [`field`].

mod field;
mod light;
mod rocks;
mod shore;
mod sky;
mod water;

/// The image's size, in its own units.
pub(crate) const VIEW: (f64, f64) = (WIDTH, HEIGHT);

const WIDTH: f64 = 3160.0;
const HEIGHT: f64 = 790.0;
/// The lighthouse's axis; the near shore is measured from it.
const AXIS: f64 = 2576.0;
/// The far waterline, and the waterline at the rocks' foot.
const SEA: f64 = 545.0;
const STRAND: f64 = 597.0;
/// The tower: its foot, the band above its plinth, and its neck under the
/// gallery, each with half its width there.
const FOOT: f64 = 513.0;
const FOOT_HALF: f64 = 46.0;
const BAND: f64 = 487.0;
const NECK: f64 = 317.0;
const NECK_HALF: f64 = 30.0;
/// The gallery: its deck's underside and top, and its railing's top.
const DECK: f64 = 303.0;
const DECK_TOP: f64 = 298.0;
const DECK_HALF: f64 = 41.5;
const RAIL: f64 = 277.0;
const RAIL_HALF: f64 = 40.0;
/// The room under the lantern, and the lantern's glass.
const ROOM_HALF: f64 = 21.5;
const GLASS_FOOT: f64 = 274.0;
const GLASS_TOP: f64 = 244.0;
const GLASS_HALF: f64 = 22.0;
/// The roof: eaves, dome, the ball on it and the spike's tip.
const EAVES: f64 = 240.0;
const EAVES_HALF: f64 = 26.5;
const DOME: f64 = 224.0;
const DOME_HALF: f64 = 25.0;
const BALL: f64 = 219.5;
const BALL_R: f64 = 4.5;
const SPIKE: f64 = 188.0;
/// The lamp's height.
const LAMP: f64 = 259.0;
/// The block the tower stands on.
const PLINTH_LEFT: f64 = -58.0;
const PLINTH_RIGHT: f64 = 12.0;
const PLINTH_FOOT: f64 = 541.0;
/// The pier: where it begins, its top and foot there, how fast each
/// climbs or sinks to the right, its cap's depth and its blocks' length.
const PIER: f64 = 9.0;
const PIER_TOP: f64 = 527.0;
const PIER_RISE: f64 = 0.0717;
const PIER_FOOT: f64 = 592.0;
const PIER_SINK: f64 = 0.035;
const CAP: f64 = 11.0;
const JOINT: f64 = 47.0;

/// Every measure above, by the name the shader gives it.
#[cfg(test)]
pub(super) const MEASURES: [(&str, f64); 36] = [
    ("TALL", HEIGHT),
    ("AXIS", AXIS),
    ("SEA", SEA),
    ("STRAND", STRAND),
    ("FOOT", FOOT),
    ("FOOT_HALF", FOOT_HALF),
    ("BAND", BAND),
    ("NECK", NECK),
    ("NECK_HALF", NECK_HALF),
    ("DECK", DECK),
    ("DECK_TOP", DECK_TOP),
    ("DECK_HALF", DECK_HALF),
    ("RAIL", RAIL),
    ("RAIL_HALF", RAIL_HALF),
    ("ROOM_HALF", ROOM_HALF),
    ("GLASS_FOOT", GLASS_FOOT),
    ("GLASS_TOP", GLASS_TOP),
    ("GLASS_HALF", GLASS_HALF),
    ("EAVES", EAVES),
    ("EAVES_HALF", EAVES_HALF),
    ("DOME", DOME),
    ("DOME_HALF", DOME_HALF),
    ("BALL", BALL),
    ("BALL_R", BALL_R),
    ("SPIKE", SPIKE),
    ("LAMP", LAMP),
    ("PLINTH_LEFT", PLINTH_LEFT),
    ("PLINTH_RIGHT", PLINTH_RIGHT),
    ("PLINTH_FOOT", PLINTH_FOOT),
    ("PIER", PIER),
    ("PIER_TOP", PIER_TOP),
    ("PIER_RISE", PIER_RISE),
    ("PIER_FOOT", PIER_FOOT),
    ("PIER_SINK", PIER_SINK),
    ("CAP", CAP),
    ("JOINT", JOINT),
];

/// The far range, the nearer hill, the rocks' crest and the farthest
/// range: each a line through its points, the rocks' from the axis and
/// the others across the picture. Each is named by where it starts in
/// [`LINE`] and how many points it has.
const FAR: (usize, usize) = (0, 17);
const NEAR: (usize, usize) = (17, 12);
const ROCKS: (usize, usize) = (29, 18);
const REMOTE: (usize, usize) = (47, 11);
pub(super) const LINE: [(f64, f64); 58] = [
    (0.0, 549.0),
    (560.0, 549.0),
    (760.0, 538.0),
    (980.0, 533.0),
    (1220.0, 539.0),
    (1420.0, 549.0),
    (1795.0, 549.0),
    (1875.0, 526.0),
    (1955.0, 506.0),
    (2035.0, 492.0),
    (2155.0, 478.0),
    (2265.0, 461.0),
    (2355.0, 471.0),
    (2475.0, 488.0),
    (2595.0, 500.0),
    (2843.0, 512.0),
    (3160.0, 520.0),
    (0.0, 549.0),
    (2275.0, 549.0),
    (2315.0, 523.0),
    (2395.0, 505.0),
    (2465.0, 490.0),
    (2535.0, 478.0),
    (2622.0, 473.0),
    (2695.0, 456.0),
    (2785.0, 442.0),
    (2875.0, 446.0),
    (2995.0, 470.0),
    (3160.0, 498.0),
    (-313.0, 598.0),
    (-297.0, 590.0),
    (-271.0, 582.0),
    (-236.0, 570.0),
    (-206.0, 556.0),
    (-181.0, 547.0),
    (-151.0, 537.0),
    (-131.0, 530.0),
    (-106.0, 521.0),
    (-89.0, 515.0),
    (-73.0, 514.0),
    (-58.0, 519.0),
    (-46.0, 541.0),
    (9.0, 543.0),
    (24.0, 556.0),
    (44.0, 568.0),
    (64.0, 584.0),
    (77.0, 598.0),
    (0.0, 549.0),
    (1180.0, 549.0),
    (1290.0, 540.0),
    (1400.0, 531.0),
    (1510.0, 524.0),
    (1620.0, 519.0),
    (1730.0, 514.0),
    (1840.0, 509.0),
    (1950.0, 503.0),
    (2300.0, 505.0),
    (3160.0, 530.0),
];

/// A colour, each part from 0 to 255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rgb(pub(super) f64, pub(super) f64, pub(super) f64);

impl Rgb {
    fn mix(self, other: Self, t: f64) -> Self {
        Self(
            field::mix(self.0, other.0, t),
            field::mix(self.1, other.1, t),
            field::mix(self.2, other.2, t),
        )
    }

    fn scaled(self, by: f64) -> Self {
        Self(self.0 * by, self.1 * by, self.2 * by)
    }

    /// As the picture writes a colour: `#rrggbb`, each part held within
    /// what two digits can say.
    fn hex(self) -> String {
        let part = |value: f64| value.round().clamp(0.0, 255.0) as u8;
        format!(
            "#{:02x}{:02x}{:02x}",
            part(self.0),
            part(self.1),
            part(self.2)
        )
    }
}

const WHITE: Rgb = Rgb(255.0, 255.0, 255.0);
const SKY_TOP: Rgb = Rgb(5.0, 19.0, 32.0);
const SKY_MID: Rgb = Rgb(12.0, 30.0, 47.0);
const SKY_LOW: Rgb = Rgb(21.0, 41.0, 61.0);
const SKY_RIM: Rgb = Rgb(34.0, 53.0, 73.0);
const HAZE: Rgb = Rgb(75.0, 84.0, 99.0);
const CLOUD: Rgb = Rgb(58.0, 77.0, 102.0);
const STAR_COOL: Rgb = Rgb(190.0, 214.0, 255.0);
const STAR_WARM: Rgb = Rgb(255.0, 232.0, 200.0);
const REMOTE_LAND: Rgb = Rgb(26.0, 44.0, 63.0);
const FAR_LAND: Rgb = Rgb(22.0, 38.0, 56.0);
const NEAR_LAND: Rgb = Rgb(16.0, 30.0, 46.0);
const MIST: Rgb = Rgb(44.0, 58.0, 76.0);
const TOWN_WARM: Rgb = Rgb(255.0, 214.0, 150.0);
const TOWN_PALE: Rgb = Rgb(255.0, 236.0, 200.0);
const SEA_FAR: Rgb = Rgb(22.0, 41.0, 60.0);
const SEA_MID: Rgb = Rgb(13.0, 30.0, 46.0);
const SEA_DEEP: Rgb = Rgb(6.0, 19.0, 33.0);
const SEA_SHEEN: Rgb = Rgb(121.0, 147.0, 176.0);
const SEA_SHADE: Rgb = Rgb(7.0, 12.0, 18.0);
const GOLD: Rgb = Rgb(251.0, 226.0, 178.0);
const GOLD_DEEP: Rgb = Rgb(214.0, 150.0, 96.0);
const PLASTER_LIT: Rgb = Rgb(248.0, 224.0, 197.0);
const PLASTER: Rgb = Rgb(66.0, 81.0, 103.0);
const PLASTER_DARK: Rgb = Rgb(48.0, 57.0, 72.0);
const IRON: Rgb = Rgb(24.0, 28.0, 36.0);
const IRON_LIT: Rgb = Rgb(106.0, 93.0, 82.0);
const GLASS: Rgb = Rgb(219.0, 182.0, 133.0);
const GLASS_DIM: Rgb = Rgb(164.0, 117.0, 77.0);
const GLASS_HOT: Rgb = Rgb(253.0, 250.0, 236.0);
const LAMPLIGHT: Rgb = Rgb(255.0, 196.0, 122.0);
const PANE: Rgb = Rgb(232.0, 197.0, 140.0);
const DOOR: Rgb = Rgb(19.0, 30.0, 46.0);
const FRAME: Rgb = Rgb(126.0, 136.0, 152.0);
const STONE: Rgb = Rgb(55.0, 66.0, 78.0);
const STONE_LIT: Rgb = Rgb(108.0, 116.0, 128.0);
const STONE_DARK: Rgb = Rgb(30.0, 36.0, 45.0);
const WALL: Rgb = Rgb(46.0, 44.0, 47.0);
const WALL_WARM: Rgb = Rgb(97.0, 80.0, 70.0);
const WALL_DARK: Rgb = Rgb(16.0, 25.0, 34.0);
const ROCK: Rgb = Rgb(51.0, 48.0, 50.0);
const ROCK_LIT: Rgb = Rgb(141.0, 132.0, 120.0);
const ROCK_DARK: Rgb = Rgb(21.0, 23.0, 28.0);

/// Every colour above but white, by the name the shader gives it.
#[cfg(test)]
pub(super) const PALETTE: [(&str, Rgb); 42] = [
    ("SKY_TOP", SKY_TOP),
    ("SKY_MID", SKY_MID),
    ("SKY_LOW", SKY_LOW),
    ("SKY_RIM", SKY_RIM),
    ("HAZE", HAZE),
    ("CLOUD", CLOUD),
    ("STAR_COOL", STAR_COOL),
    ("STAR_WARM", STAR_WARM),
    ("REMOTE_LAND", REMOTE_LAND),
    ("FAR_LAND", FAR_LAND),
    ("NEAR_LAND", NEAR_LAND),
    ("MIST", MIST),
    ("TOWN_WARM", TOWN_WARM),
    ("TOWN_PALE", TOWN_PALE),
    ("SEA_FAR", SEA_FAR),
    ("SEA_MID", SEA_MID),
    ("SEA_DEEP", SEA_DEEP),
    ("SEA_SHEEN", SEA_SHEEN),
    ("SEA_SHADE", SEA_SHADE),
    ("GOLD", GOLD),
    ("GOLD_DEEP", GOLD_DEEP),
    ("PLASTER_LIT", PLASTER_LIT),
    ("PLASTER", PLASTER),
    ("PLASTER_DARK", PLASTER_DARK),
    ("IRON", IRON),
    ("IRON_LIT", IRON_LIT),
    ("GLASS", GLASS),
    ("GLASS_DIM", GLASS_DIM),
    ("GLASS_HOT", GLASS_HOT),
    ("LAMPLIGHT", LAMPLIGHT),
    ("PANE", PANE),
    ("DOOR", DOOR),
    ("FRAME", FRAME),
    ("STONE", STONE),
    ("STONE_LIT", STONE_LIT),
    ("STONE_DARK", STONE_DARK),
    ("WALL", WALL),
    ("WALL_WARM", WALL_WARM),
    ("WALL_DARK", WALL_DARK),
    ("ROCK", ROCK),
    ("ROCK_LIT", ROCK_LIT),
    ("ROCK_DARK", ROCK_DARK),
];

/// The image, as SVG text.
pub(super) fn draw() -> String {
    let mut out = String::with_capacity(128 * 1024);
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
         width=\"{WIDTH}\" height=\"{HEIGHT}\">"
    ));
    sky::draw(&mut out);
    water::draw(&mut out);
    shore::draw(&mut out);
    rocks::draw(&mut out);
    light::draw(&mut out);
    out.push_str("</svg>");
    out
}

/// A number as the picture writes it: to one decimal place, and without
/// the place when it says nothing.
fn n(value: f64) -> String {
    let written = format!("{value:.1}");
    match written.strip_suffix(".0") {
        Some(whole) => whole.to_owned(),
        None => written,
    }
}

/// An opacity as the picture writes it: to two places.
fn dim(value: f64) -> String {
    format!("{:.2}", value.clamp(0.0, 1.0))
}

/// The points of a shape, as an SVG `points` attribute's value.
fn points(shape: &[(f64, f64)]) -> String {
    let mut out = String::with_capacity(shape.len() * 12);
    for (index, (x, y)) in shape.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{},{}", n(*x), n(*y)));
    }
    out
}

/// A filled polygon; `more` is any further attributes, each led by a space.
fn polygon(out: &mut String, shape: &[(f64, f64)], fill: &str, more: &str) {
    out.push_str(&format!(
        "<polygon points=\"{}\" fill=\"{fill}\"{more}/>",
        points(shape)
    ));
}

/// A filled rectangle with corners at (`x0`, `y0`) and (`x1`, `y1`).
fn rect(out: &mut String, (x0, y0): (f64, f64), (x1, y1): (f64, f64), fill: &str, more: &str) {
    out.push_str(&format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\"{more}/>",
        n(x0),
        n(y0),
        n(x1 - x0),
        n(y1 - y0)
    ));
}

/// A gradient of `stops`, each an offset, a colour and an opacity. `kind`
/// is the element's name with its attributes.
fn gradient(out: &mut String, kind: &str, stops: &[(f64, Rgb, f64)]) {
    let name = kind.split(' ').next().unwrap_or(kind);
    out.push_str(&format!("<{kind}>"));
    for (offset, colour, opacity) in stops {
        out.push_str(&format!(
            "<stop offset=\"{offset:.3}\" stop-color=\"{}\" stop-opacity=\"{}\"/>",
            colour.hex(),
            dim(*opacity)
        ));
    }
    out.push_str(&format!("</{name}>"));
}

#[cfg(test)]
mod tests;
