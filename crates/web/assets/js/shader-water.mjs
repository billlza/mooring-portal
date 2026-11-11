// The sea of the live scene (`shader.mjs`): dark water that mirrors the
// sky and the far shore in its ripples, holds the sky's light along them
// in long soft streaks, glitters where a ripple catches it, and carries
// the lamp's path from the rocks to the viewer.

export const WATER = `
// What the far water mirrors at a place above the far waterline: the sky
// and the hills under it, without the stars, which no ripple would hold.
vec3 mirrored(vec2 m) {
  return hills(sky(m, veil(m)), m);
}

// The water's face at a place, measured in ripples along the row and in
// rows from the far shore: ripples many times as long as they are tall,
// in two sizes that slide against each other, so that no ripple keeps its
// shape.
float ripples(vec2 at) {
  // No ripple lies level from end to end.
  float bend = 0.45 * wave(at.x / 17.0 + at.y * 0.8);
  return 0.62 * noise(vec2(at.x / 22.0 + u_time * 0.03, at.y * 1.5 + bend))
    + 0.38 * noise(vec2(at.x / 8.0 - u_time * 0.07, at.y * 3.3 + 5.0 - 1.7 * bend));
}

// Points of light on the water: each cell of a row has at most one, which
// rides the row's crest and flashes. Rows slide past each other.
float flecks(float row, float across, uint salt) {
  float line = floor(row);
  ivec2 lane = ivec2(int(line), 11);
  float slid = across * 0.33 + u_time * mix(-0.07, 0.07, hash(lane, salt)) + 31.0 * hash(lane, salt + 1u);
  ivec2 cell = ivec2(int(floor(slid)), int(line));
  float size = hash(cell, salt + 4u);
  if (hash(cell, salt + 2u) > 0.5) {
    return 0.0;
  }
  float dx = (fract(slid) - 0.3 - 0.4 * hash(cell, salt + 3u)) / mix(0.05, 0.16, size);
  float dy = (fract(row) - 0.5) / 0.15;
  float blink = smoothstep(0.3, 1.0, wave(u_time * mix(0.9, 3.0, hash(cell, salt + 5u)) + TAU * hash(cell, salt + 6u)));
  return exp(-dx * dx - dy * dy) * blink * mix(0.45, 1.0, size);
}

// How dark the water lies under the near shore: its rocks and its pier
// stand against the sky, and the water at their foot mirrors them.
float lee(vec2 q, float sway, float lift) {
  if (q.x < -330.0) {
    return 0.0;
  }
  float x = q.x + sway;
  float foot = x > 77.0 ? pierFoot(x) : STRAND;
  float under = q.y - foot;
  if (under < -4.0) {
    return 0.0;
  }
  float top = x > 77.0 ? pierTop(x)
    : x > -313.0 ? min(along(ROCKS, ROCKS_POINTS, x), x > PIER ? pierTop(x) : STRAND) : STRAND;
  float tall = foot - top;
  return smoothstep(-3.0, 3.0, tall - under - lift) * exp(-max(under, 0.0) / 70.0);
}

// The lamp's path on the water, from the rocks under the lighthouse to the
// viewer. Under the lamp nearly every ripple shows it; to either side
// only one that tilts far enough toward it does, so the path is a column
// of bands of every length, close and bright along its middle, that
// widens as it comes and never keeps still.
float lamppath(vec2 q, vec2 at, float sway) {
  float under = q.y - STRAND;
  if (under < 0.0 || abs(q.x) > 130.0) {
    return 0.0;
  }
  float wide = 9.0 + 0.17 * under;
  float off = abs(q.x - sway) / wide;
  float need = mix(0.3, 0.92, smoothstep(0.0, 1.3, off));
  float bend = 0.4 * wave(at.x / 6.0 + at.y * 1.3);
  float tilt = 0.6 * noise(vec2(at.x / 4.5 + u_time * 0.11, at.y * 2.4 + 3.0 + bend))
    + 0.4 * noise(vec2(at.x / 9.0 - u_time * 0.06, at.y * 1.1 + 11.0 - bend));
  float strength = smoothstep(2.0, 26.0, under) * (1.0 - 0.6 * smoothstep(80.0, 200.0, under));
  return smoothstep(need, need + 0.3, tilt) * strength;
}

vec3 sea(vec2 p, float angle) {
  vec2 q = vec2(p.x - AXIS, p.y);
  float depth = clamp((p.y - SEA) / (TALL - SEA), 0.0, 1.0);
  vec3 c = depth < 0.3 ? mix(SEA_FAR, SEA_MID, depth / 0.3) : mix(SEA_MID, SEA_DEEP, (depth - 0.3) / 0.7);
  float t = u_time;
  // How long a ripple is on the screen: short by the far shore, long near
  // the viewer, and never under a couple of pixels, so the water does not
  // shimmer. The rows of ripples are counted from the far shore, and come
  // toward the viewer.
  float ripple = max(1.4, 2.0 * u_unit) + 8.5 * depth;
  float row = 28.8 * log(ripple) - 0.3 * t;
  float across = q.x / ripple;
  float calm = smoothstep(0.0, 0.04, depth);
  // Each row slides what it mirrors to one side, and no two rows agree;
  // under them a slow swell lifts the whole.
  float sway = 0.6 * wave(row * 1.9 + t * 1.05 + across * 0.12)
    + 0.3 * wave(row * 4.3 - t * 1.6 + across * 0.35)
    + 0.5 * (noise(vec2(across * 0.16 + t * 0.05, row * 0.6 - t * 0.2)) - 0.5);
  float lift = wave(row * 2.7 + t * 0.8 + across * 0.5);
  float swell = wave(row * 0.21 - t * 0.45 + across * 0.015 + 2.0 * noise(vec2(p.x / 260.0, t * 0.03)));
  vec2 m = vec2(p.x + (sway * ripple * 1.1 + swell * 9.0 * depth) * calm,
    min(2.0 * SEA - p.y + (lift * ripple * 0.5 + swell * 5.0 * depth) * calm, SEA - 0.5));
  vec3 image = mirrored(m);
  // Far water mirrors nearly all it faces. Near water is seen more
  // steeply, and shows its own dark.
  c = mix(c, image, mix(0.8, 0.08, smoothstep(0.0, 0.8, depth)));
  // The near shore's dark in the water at its foot.
  float dark = lee(q, sway * ripple * 0.9, lift * ripple * 0.8);
  // Where a ripple turns its face to the sky it holds the sky's light,
  // more where the swell opens the water to it; where it turns away it
  // is a little darker.
  float face = ripples(vec2(across, row));
  float held = smoothstep(0.44, 0.84, face);
  float open = smoothstep(0.25, 0.8, noise(vec2(p.x / 380.0 + t * 0.012, row / 6.0)));
  float aside = q.x / 430.0;
  float glow = exp(-aside * aside);
  c *= 1.0 - 0.16 * (1.0 - smoothstep(0.25, 0.5, face)) * calm;
  c += SEA_SHEEN * held * (0.25 + 0.75 * open) * (0.3 + 0.7 * glow) * mix(0.2, 0.09, depth) * calm * (1.0 - 0.8 * dark);
  // The town's lamps, drawn out down the water and broken by the ripples.
  float under = p.y - SEA;
  if (under < 70.0) {
    c += lamps(vec2(p.x + sway * ripple * 0.8, SEA - 0.16 * under), 2.6)
      * exp(-under / 22.0) * (0.35 + 0.65 * smoothstep(0.3, 0.7, face));
  }
  c = mix(c, SEA_SHADE, 0.72 * dark);
  // Glitter: points where a ripple catches the haze's light, most under
  // the lighthouse, where the haze is brightest, more along a streak, and
  // faint everywhere else.
  float lit = glow * (0.3 + 0.7 * smoothstep(0.02, 0.3, depth)) * (1.0 - 0.8 * dark);
  float glitter = flecks(row, across, 101u) + 0.7 * flecks(row + 0.5, across * 0.61 + 7.0, 111u);
  c += SEA_SHEEN * glitter * (0.05 + 0.55 * lit) * (0.4 + 1.1 * held) * calm;
  // The lamp's path, brighter as a beam swings toward the viewer, in the
  // glow it lays on the water around it.
  float facing = sin(angle) * sin(angle);
  float path = lamppath(q, vec2(across, row), sway * ripple * 0.6);
  float near = exp(-q.x * q.x / (55.0 * 55.0 + 0.6 * under * under)) * smoothstep(STRAND - SEA, STRAND - SEA + 30.0, under);
  c = mix(c, mix(GOLD_DEEP, GOLD, path), path * (0.72 + 0.28 * facing * facing));
  c += GOLD_DEEP * near * (0.07 + 0.2 * held + 0.5 * glitter) * (0.7 + 0.5 * facing);
  return c;
}
`;
