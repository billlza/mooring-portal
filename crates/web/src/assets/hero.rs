//! The home page's hero image, drawn by code: a starry sky, a ring of star
//! trails around the celestial pole, snowy ridges, a still harbour that
//! reflects them, and a boat moored at a jetty whose lantern is the one
//! warm light in the scene.
//!
//! The drawing is deterministic (a fixed seed), so the image, and with it
//! its content-hashed address, changes only when this code does. It uses
//! presentation attributes only, no `<style>` or script, so the content
//! security policy has nothing to allow.

/// The waterline.
const SHORE: f64 = 800.0;
/// The ring's centre and radius.
const RING_X: f64 = 1100.0;
const RING_Y: f64 = 500.0;
const RING_R: f64 = 280.0;
/// The boat's mast, whose top carries the lantern.
const MAST_X: f64 = 1215.0;
const LANTERN_Y: f64 = 768.0;
/// "moorligh", as a seed.
const SEED: u64 = 0x6d6f_6f72_6c69_6768;

/// The image as SVG text.
pub(super) fn svg() -> String {
    let mut rng = Rng(SEED);
    let mut out = String::with_capacity(64 * 1024);
    out.push_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1600 1000\" \
         width=\"1600\" height=\"1000\" preserveAspectRatio=\"xMidYMid slice\">",
    );
    defs(&mut out, &mut rng);
    out.push_str("<rect width=\"1600\" height=\"1000\" fill=\"url(#sky)\"/>");
    out.push_str(
        "<ellipse cx=\"820\" cy=\"300\" rx=\"900\" ry=\"130\" fill=\"url(#milky)\" \
         transform=\"rotate(-16 820 300)\"/>",
    );
    stars(&mut out, &mut rng);
    out.push_str("<use href=\"#ring\"/><use href=\"#ridges\"/>");
    water(&mut out, &mut rng);
    harbour(&mut out);
    out.push_str("</svg>");
    out
}

fn defs(out: &mut String, rng: &mut Rng) {
    out.push_str(concat!(
        "<defs>",
        "<linearGradient id=\"sky\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">",
        "<stop offset=\"0\" stop-color=\"#01030b\"/><stop offset=\"0.5\" stop-color=\"#050d2a\"/>",
        "<stop offset=\"0.8\" stop-color=\"#0d2459\"/></linearGradient>",
        "<radialGradient id=\"milky\"><stop offset=\"0\" stop-color=\"#c9d9ff\" stop-opacity=\"0.13\"/>",
        "<stop offset=\"1\" stop-color=\"#c9d9ff\" stop-opacity=\"0\"/></radialGradient>",
        "<radialGradient id=\"halo\"><stop offset=\"0.72\" stop-color=\"#3d8bff\" stop-opacity=\"0\"/>",
        "<stop offset=\"0.86\" stop-color=\"#3d8bff\" stop-opacity=\"0.3\"/>",
        "<stop offset=\"1\" stop-color=\"#3d8bff\" stop-opacity=\"0\"/></radialGradient>",
        "<radialGradient id=\"core\"><stop offset=\"0\" stop-color=\"#1b3f8f\" stop-opacity=\"0.5\"/>",
        "<stop offset=\"1\" stop-color=\"#1b3f8f\" stop-opacity=\"0\"/></radialGradient>",
        "<linearGradient id=\"snow\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"570\" x2=\"0\" y2=\"760\">",
        "<stop offset=\"0\" stop-color=\"#f6f9ff\"/><stop offset=\"0.3\" stop-color=\"#c6d7f5\"/>",
        "<stop offset=\"1\" stop-color=\"#6f8cc8\"/></linearGradient>",
        "<linearGradient id=\"rock\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"600\" x2=\"0\" y2=\"800\">",
        "<stop offset=\"0\" stop-color=\"#1f356c\"/><stop offset=\"1\" stop-color=\"#081230\"/></linearGradient>",
        "<linearGradient id=\"far\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"560\" x2=\"0\" y2=\"800\">",
        "<stop offset=\"0\" stop-color=\"#35518f\"/><stop offset=\"1\" stop-color=\"#0f1c44\"/></linearGradient>",
        "<linearGradient id=\"mist\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">",
        "<stop offset=\"0\" stop-color=\"#4f86e8\" stop-opacity=\"0\"/><stop offset=\"0.6\" stop-color=\"#4f86e8\" stop-opacity=\"0.18\"/>",
        "<stop offset=\"1\" stop-color=\"#4f86e8\" stop-opacity=\"0\"/></linearGradient>",
        "<linearGradient id=\"shore\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">",
        "<stop offset=\"0\" stop-color=\"#0a1430\"/><stop offset=\"1\" stop-color=\"#02040b\"/></linearGradient>",
        "<linearGradient id=\"water\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">",
        "<stop offset=\"0\" stop-color=\"#0a1a45\"/><stop offset=\"1\" stop-color=\"#01040c\"/></linearGradient>",
        "<radialGradient id=\"lantern\"><stop offset=\"0\" stop-color=\"#ffd27a\" stop-opacity=\"0.9\"/>",
        "<stop offset=\"0.35\" stop-color=\"#ffb347\" stop-opacity=\"0.32\"/>",
        "<stop offset=\"1\" stop-color=\"#ffb347\" stop-opacity=\"0\"/></radialGradient>",
        "<filter id=\"glow\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"><feGaussianBlur stdDeviation=\"9\"/></filter>",
        "<filter id=\"soft\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\"><feGaussianBlur stdDeviation=\"2.5\"/></filter>",
        "<filter id=\"blur\" x=\"-5%\" y=\"-5%\" width=\"110%\" height=\"110%\"><feGaussianBlur stdDeviation=\"4 1.5\"/></filter>",
        "<clipPath id=\"below\"><rect x=\"0\" y=\"800\" width=\"1600\" height=\"200\"/></clipPath>",
    ));
    ring(out, rng);
    ridges(out, rng);
    out.push_str("</defs>");
}

fn stars(out: &mut String, rng: &mut Rng) {
    out.push_str("<g>");
    for _ in 0..230 {
        let x = rng.range(0.0, 1600.0);
        let y = rng.range(0.0, 760.0);
        let size = rng.unit();
        let radius = 0.35 + 1.3 * size * size * size;
        let opacity = rng.range(0.35, 1.0);
        let colour = match rng.unit() {
            tint if tint < 0.12 => "#cfe0ff",
            tint if tint < 0.2 => "#ffe7c2",
            _ => "#ffffff",
        };
        out.push_str(&format!("<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"{radius:.2}\" fill=\"{colour}\" opacity=\"{opacity:.2}\"/>"
        ));
    }
    for _ in 0..12 {
        let x = rng.range(0.0, 1600.0);
        let y = rng.range(0.0, 620.0);
        out.push_str(&format!("<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"7\" fill=\"#ffffff\" opacity=\"0.14\" filter=\"url(#soft)\"/>\
             <circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"1.9\" fill=\"#ffffff\"/>"
        ));
    }
    out.push_str("</g>");
}

/// Star trails around the pole, inside a neon ring.
fn ring(out: &mut String, rng: &mut Rng) {
    let (x, y, r) = (RING_X, RING_Y, RING_R);
    out.push_str(&format!("<g id=\"ring\"><circle cx=\"{x}\" cy=\"{y}\" r=\"{}\" fill=\"url(#halo)\"/>\
         <circle cx=\"{x}\" cy=\"{y}\" r=\"{}\" fill=\"url(#core)\"/><g fill=\"none\" stroke-linecap=\"round\">",
        r + 70.0,
        r - 10.0
    ));
    for index in 0..34_u32 {
        let radius = 40.0 + f64::from(index) * 7.2;
        let dash = rng.range(18.0, 150.0);
        let gap = rng.range(6.0, 70.0);
        let turn = rng.range(0.0, 360.0);
        let opacity = rng.range(0.14, 0.55);
        let width = rng.range(0.7, 1.8);
        let colour = if rng.unit() < 0.6 {
            "#6fb6ff"
        } else {
            "#dceaff"
        };
        out.push_str(&format!("<circle cx=\"{x}\" cy=\"{y}\" r=\"{radius:.1}\" stroke=\"{colour}\" stroke-opacity=\"{opacity:.2}\" \
             stroke-width=\"{width:.2}\" stroke-dasharray=\"{dash:.1} {gap:.1}\" transform=\"rotate({turn:.1} {x} {y})\"/>"
        ));
    }
    out.push_str(&format!("</g><circle cx=\"{x}\" cy=\"{y}\" r=\"{r}\" fill=\"none\" stroke=\"#5aa9ff\" stroke-width=\"16\" \
         stroke-opacity=\"0.55\" filter=\"url(#glow)\"/>\
         <circle cx=\"{x}\" cy=\"{y}\" r=\"{r}\" fill=\"none\" stroke=\"#8fd0ff\" stroke-width=\"5\"/>\
         <circle cx=\"{x}\" cy=\"{y}\" r=\"{r}\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"1.6\" stroke-opacity=\"0.9\"/>\
         <circle cx=\"{x}\" cy=\"{y}\" r=\"{}\" fill=\"none\" stroke=\"#4d8dff\" stroke-width=\"1.2\" stroke-opacity=\"0.55\"/>\
         <circle cx=\"{x}\" cy=\"{y}\" r=\"9\" fill=\"#ffffff\" opacity=\"0.25\" filter=\"url(#soft)\"/>\
         <circle cx=\"{x}\" cy=\"{y}\" r=\"2.6\" fill=\"#ffffff\"/></g>",
        r - 24.0
    ));
}

/// Two ranges: a far, hazy one, and a near one whose crest is snow and
/// whose slopes below an uneven snow line are bare rock, with a valley
/// under the ring.
fn ridges(out: &mut String, rng: &mut Rng) {
    let far = ridge(
        rng,
        &[
            (0.0, 640.0),
            (200.0, 588.0),
            (430.0, 642.0),
            (650.0, 578.0),
            (900.0, 648.0),
            (1150.0, 602.0),
            (1380.0, 566.0),
            (1600.0, 624.0),
        ],
        24.0,
    );
    let near = ridge(
        rng,
        &[
            (0.0, 736.0),
            (230.0, 660.0),
            (470.0, 716.0),
            (640.0, 676.0),
            (880.0, 604.0),
            (1080.0, 742.0),
            (1260.0, 696.0),
            (1420.0, 580.0),
            (1600.0, 706.0),
        ],
        30.0,
    );
    // Snow lies above an uneven snow line, so the high peaks are white
    // fields and the low ridges only dusted along their crest.
    let (phase, wobble) = (
        rng.range(0.0, std::f64::consts::TAU),
        rng.range(0.0, std::f64::consts::TAU),
    );
    let snow_line: Vec<(f64, f64)> = near
        .iter()
        .map(|&(x, y)| {
            let line = 676.0 + 18.0 * (x / 53.0 + phase).sin() + 8.0 * (x / 17.0 + wobble).sin();
            let dusting = y + 5.0 + rng.range(0.0, 3.0);
            (x, line.max(dusting))
        })
        .collect();
    out.push_str("<g id=\"ridges\">");
    polygon(out, &far, "url(#far)");
    polygon(out, &near, "url(#snow)");
    polygon(out, &snow_line, "url(#rock)");
    out.push_str("<polyline fill=\"none\" stroke=\"#ffffff\" stroke-opacity=\"0.45\" stroke-width=\"1.1\" points=\"");
    points(out, &near);
    out.push_str(
        "\"/><rect x=\"0\" y=\"730\" width=\"1600\" height=\"80\" fill=\"url(#mist)\"/></g>",
    );
}

/// A ridge line through `peaks`, roughened by midpoint displacement.
fn ridge(rng: &mut Rng, peaks: &[(f64, f64)], roughness: f64) -> Vec<(f64, f64)> {
    let mut line = peaks.to_vec();
    let mut spread = roughness;
    for _ in 0..5 {
        let mut finer = Vec::with_capacity(line.len() * 2);
        for pair in line.windows(2) {
            if let [(x0, y0), (x1, y1)] = *pair {
                finer.push((x0, y0));
                finer.push((
                    (x0 + x1) / 2.0,
                    (y0 + y1) / 2.0 + rng.range(-spread, spread),
                ));
            }
        }
        finer.extend(line.last().copied());
        line = finer;
        spread *= 0.55;
    }
    line
}

fn polygon(out: &mut String, line: &[(f64, f64)], fill: &str) {
    out.push_str(&format!("<polygon fill=\"{fill}\" points=\"0,{SHORE} "));
    points(out, line);
    out.push_str(&format!(" 1600,{SHORE}\"/>"));
}

fn points(out: &mut String, line: &[(f64, f64)]) {
    for (index, (x, y)) in line.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{x:.1},{y:.1}"));
    }
}

/// The harbour: still water with the ring and ridges reflected in it, and
/// light on the surface.
fn water(out: &mut String, rng: &mut Rng) {
    out.push_str(
        "<rect x=\"0\" y=\"800\" width=\"1600\" height=\"200\" fill=\"url(#water)\"/>\
         <g clip-path=\"url(#below)\" opacity=\"0.24\"><g transform=\"matrix(1 0 0 -1 0 1600)\" \
         filter=\"url(#blur)\"><use href=\"#ring\"/><use href=\"#ridges\"/></g></g><g>",
    );
    for _ in 0..40 {
        let x = RING_X + rng.range(-260.0, 260.0);
        let y = rng.range(812.0, 990.0);
        let width = rng.range(20.0, 140.0);
        let opacity = rng.range(0.06, 0.24);
        out.push_str(&format!("<rect x=\"{:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"1.2\" fill=\"#9fd0ff\" opacity=\"{opacity:.2}\"/>",
            x - width / 2.0
        ));
    }
    for _ in 0..10 {
        let x = MAST_X + rng.range(-24.0, 24.0);
        let y = rng.range(818.0, 900.0);
        let width = rng.range(8.0, 30.0);
        out.push_str(&format!("<rect x=\"{:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"1.2\" fill=\"#ffc46b\" opacity=\"0.35\"/>",
            x - width / 2.0
        ));
    }
    out.push_str("</g>");
}

/// The near shore, the jetty, the moored boat and its lantern, and a
/// small figure on the jetty looking up at the ring.
fn harbour(out: &mut String) {
    out.push_str(
        "<path d=\"M0 812 C140 806 260 816 360 826 C430 832 480 838 520 850 L520 1000 L0 1000 Z\" fill=\"url(#shore)\"/>\
         <path d=\"M1330 804 C1420 794 1520 790 1600 788 L1600 1000 L1330 1000 Z\" fill=\"url(#shore)\"/>\
         <rect x=\"1150\" y=\"804\" width=\"190\" height=\"3\" fill=\"#0a1124\"/>",
    );
    for post in [1160, 1200, 1240, 1280, 1320] {
        out.push_str(&format!(
            "<rect x=\"{post}\" y=\"806\" width=\"3\" height=\"16\" fill=\"#070d1e\"/>"
        ));
    }
    out.push_str(&format!("<path d=\"M1170 806 L1252 806 L1242 818 Q1210 823 1180 818 Z\" fill=\"#050a18\"/>\
         <rect x=\"1196\" y=\"798\" width=\"22\" height=\"8\" fill=\"#071029\"/>\
         <line x1=\"{MAST_X}\" y1=\"806\" x2=\"{MAST_X}\" y2=\"{}\" stroke=\"#0b142c\" stroke-width=\"1.6\"/>\
         <ellipse cx=\"{MAST_X}\" cy=\"852\" rx=\"6\" ry=\"42\" fill=\"url(#lantern)\" opacity=\"0.55\"/>\
         <circle cx=\"{MAST_X}\" cy=\"{LANTERN_Y}\" r=\"36\" fill=\"url(#lantern)\"/>\
         <circle cx=\"{MAST_X}\" cy=\"{LANTERN_Y}\" r=\"2.4\" fill=\"#fff1c8\"/>",
        LANTERN_Y + 2.0
    ));
    out.push_str(
        "<g fill=\"#04070f\"><circle cx=\"1262\" cy=\"786\" r=\"3.2\"/>\
         <path d=\"M1258.5 790 L1265.5 790 L1266.5 800 L1264.6 800 L1264 806 L1260 806 L1259.4 800 L1257.5 800 Z\"/></g>\
         <path d=\"M1258.5 790 L1257.5 800\" stroke=\"#6fb6ff\" stroke-opacity=\"0.6\" stroke-width=\"0.6\"/>",
    );
}

/// SplitMix64: small, fast and fully determined by its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        // The top 53 bits fill an f64's mantissa exactly.
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_is_deterministic_and_self_contained() {
        let first = svg();
        assert_eq!(first, svg());
        assert!(first.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(first.ends_with("</svg>"));
        // Nothing is fetched from elsewhere, and there is nothing to run.
        assert_eq!(first.matches("http").count(), 1, "only the namespace");
        for forbidden in [
            "<script",
            "<style",
            "<image",
            "style=",
            "xlink",
            "<foreignObject",
        ] {
            assert!(!first.contains(forbidden), "{forbidden}");
        }
        assert!(first.len() < 80_000, "{} bytes", first.len());
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
}
