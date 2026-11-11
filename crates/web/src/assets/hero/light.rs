//! The lamp's light in the air of the drawn image, as the live scene's
//! `lamplight` throws it when the scene's clock reads nothing: a glow
//! about the lantern, and its two beams lying across the view.

use super::{AXIS, LAMP, LAMPLIGHT, Rgb, gradient, n, polygon};

pub(super) fn draw(out: &mut String) {
    const REACH: f64 = 700.0;
    out.push_str("<defs>");
    let glow = |r: f64| 0.42 * (-r / 15.0).exp() + 0.07 * (-r / 64.0).exp();
    let stops: Vec<(f64, Rgb, f64)> = [0.0, 8.0, 15.0, 30.0, 60.0, 110.0, 180.0, 240.0]
        .into_iter()
        .map(|r| (r / 240.0, LAMPLIGHT, if r < 240.0 { glow(r) } else { 0.0 }))
        .collect();
    gradient(out, "radialGradient id=\"glow\"", &stops);
    // A beam's light along its length, as the shader's falls away. Across
    // it the shader's beam is brightest in the middle; here it is three
    // beams of one length laid over each other, the narrowest the last.
    const LAYERS: [(f64, f64); 3] = [(1.5, 0.2), (0.9, 0.3), (0.45, 0.4)];
    for (layer, (_, share)) in LAYERS.into_iter().enumerate() {
        let stops: Vec<(f64, Rgb, f64)> = [0.0, 20.0, 60.0, 120.0, 200.0, 320.0, 480.0, REACH]
            .into_iter()
            .map(|reach| {
                let light = 1.5 * (-reach / 380.0).exp() / (1.0 + reach / 60.0);
                let left = if reach < REACH {
                    share * light.min(1.0)
                } else {
                    0.0
                };
                (reach / REACH, LAMPLIGHT, left)
            })
            .collect();
        for (side, toward) in [("left", -1.0), ("right", 1.0)] {
            gradient(
                out,
                &format!(
                    "linearGradient id=\"beam-{side}-{layer}\" gradientUnits=\"userSpaceOnUse\" x1=\"{AXIS}\" y1=\"0\" x2=\"{}\" y2=\"0\"",
                    n(AXIS + toward * REACH)
                ),
                &stops,
            );
        }
    }
    out.push_str(
        "<filter id=\"soften\" x=\"-5%\" y=\"-60%\" width=\"110%\" height=\"220%\">\
         <feGaussianBlur stdDeviation=\"2 2.5\"/></filter></defs>",
    );
    out.push_str("<g filter=\"url(#soften)\">");
    for (layer, (width, _)) in LAYERS.into_iter().enumerate() {
        for (side, toward) in [("left", -1.0), ("right", 1.0)] {
            polygon(
                out,
                &[
                    (AXIS, LAMP - 4.5 * width),
                    (AXIS + toward * REACH, LAMP - width * (4.5 + 0.045 * REACH)),
                    (AXIS + toward * REACH, LAMP + width * (4.5 + 0.045 * REACH)),
                    (AXIS, LAMP + 4.5 * width),
                ],
                &format!("url(#beam-{side}-{layer})"),
                "",
            );
        }
    }
    out.push_str(&format!(
        "</g><circle cx=\"{AXIS}\" cy=\"{LAMP}\" r=\"240\" fill=\"url(#glow)\"/>"
    ));
}
