// The sea of the live scene (`shader.mjs`): dark water that mirrors the
// sky and the far shore in its ripples, glitters where a ripple catches
// the light, and carries the lamp's path from the rocks to the viewer.

export const WATER = `
// What the far water mirrors at a place above the far waterline: the sky
// and the hills under it, without the stars, which no ripple would hold.
vec3 mirrored(vec2 m) {
  return hills(sky(m, veil(m)), m);
}

// Flecks of light on the water: each cell of a row has at most one, which
// lies along the row and comes and goes. Rows slide past each other.
float flecks(float row, float across, uint salt) {
  float line = floor(row);
  ivec2 lane = ivec2(int(line), 11);
  float slid = across * 0.33 + u_time * mix(-0.07, 0.07, hash(lane, salt)) + 31.0 * hash(lane, salt + 1u);
  ivec2 cell = ivec2(int(floor(slid)), int(line));
  if (hash(cell, salt + 2u) > 0.5) {
    return 0.0;
  }
  float dx = (fract(slid) - 0.3 - 0.4 * hash(cell, salt + 3u)) / mix(0.1, 0.27, hash(cell, salt + 4u));
  float dy = (fract(row) - 0.5) / 0.13;
  float blink = smoothstep(0.3, 1.0, wave(u_time * mix(0.9, 3.0, hash(cell, salt + 5u)) + TAU * hash(cell, salt + 6u)));
  return exp(-dx * dx - dy * dy) * blink;
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
// viewer: a column of bright dashes, one to a ripple, that widens as it
// comes and never keeps still.
float lamppath(vec2 q, float row, float sway) {
  float under = q.y - STRAND;
  if (under < 0.0 || abs(q.x) > 130.0) {
    return 0.0;
  }
  float wide = 9.0 + 0.17 * under;
  ivec2 id = ivec2(int(floor(row)), 9);
  float beat = wave(u_time * mix(0.5, 1.4, hash(id, 91u)) + TAU * hash(id, 92u));
  float reach = wide * (mix(0.3, 1.0, hash(id, 93u)) + 0.22 * beat);
  float off = wide * 0.4 * (hash(id, 94u) - 0.5 + 0.4 * wave(u_time * 0.7 + TAU * hash(id, 95u))) + sway;
  // Some ripples lie close enough to run into the next.
  float thick = mix(0.26, 0.5, hash(id, 97u));
  float dash = (1.0 - smoothstep(0.6 * reach, reach, abs(q.x - off)))
    * (1.0 - smoothstep(thick - 0.12, thick, abs(fract(row) - 0.5)));
  // A smaller glint beside it, on one side or the other.
  float aside = off + wide * mix(0.9, 1.7, hash(id, 98u)) * (hash(id, 99u) < 0.5 ? -1.0 : 1.0);
  float glint = (1.0 - smoothstep(0.1 * wide, 0.32 * wide, abs(q.x - aside)))
    * (1.0 - smoothstep(0.1, 0.22, abs(fract(row) - 0.5))) * smoothstep(0.1, 0.9, beat);
  float strength = smoothstep(2.0, 26.0, under) * (1.0 - 0.6 * smoothstep(80.0, 200.0, under));
  return (dash + 0.6 * glint) * strength * mix(0.5, 1.0, hash(id, 96u));
}

vec3 sea(vec2 p, float angle) {
  vec2 q = vec2(p.x - AXIS, p.y);
  float depth = clamp((p.y - SEA) / (TALL - SEA), 0.0, 1.0);
  vec3 c = depth < 0.3 ? mix(SEA_FAR, SEA_MID, depth / 0.3) : mix(SEA_MID, SEA_DEEP, (depth - 0.3) / 0.7);
  float t = u_time;
  // How long a ripple is on the screen: short by the far shore, long near
  // the viewer, and never under a couple of pixels, so the water does not
  // shimmer. The rows of ripples are counted from the far shore.
  float ripple = max(1.4, 2.0 * u_unit) + 8.5 * depth;
  float row = 28.8 * log(ripple);
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
  // The far face of each ripple takes a little of the sky's light.
  c += SEA_SHEEN * 0.05 * (0.5 + 0.5 * lift) * (0.35 + 0.65 * (0.5 + 0.5 * swell)) * calm * (1.0 - 0.5 * depth);
  // The town's lamps, drawn out down the water and broken by the ripples.
  float under = p.y - SEA;
  if (under < 70.0) {
    c += lamps(vec2(p.x + sway * ripple * 0.8, SEA - 0.16 * under), 2.6)
      * exp(-under / 22.0) * (0.55 + 0.45 * lift);
  }
  // The near shore's dark in the water at its foot.
  float dark = lee(q, sway * ripple * 0.9, lift * ripple * 0.8);
  c = mix(c, SEA_SHADE, 0.72 * dark);
  // Glitter: flecks where a ripple catches the haze's light, most under
  // the lighthouse, where the haze is brightest, and faint everywhere
  // else.
  float aside = q.x / 430.0;
  float lit = exp(-aside * aside) * (0.3 + 0.7 * smoothstep(0.02, 0.3, depth)) * (1.0 - 0.8 * dark);
  float glitter = flecks(row, across, 101u) + 0.7 * flecks(row + 0.5, across * 0.61 + 7.0, 111u);
  c += SEA_SHEEN * glitter * (0.05 + 0.55 * lit) * calm;
  // The lamp's path, brighter as a beam swings toward the viewer.
  float facing = sin(angle) * sin(angle);
  float path = lamppath(q, row, sway * ripple * 0.6);
  float near = exp(-q.x * q.x / (55.0 * 55.0 + 0.6 * under * under)) * smoothstep(STRAND - SEA, STRAND - SEA + 30.0, under);
  c += mix(GOLD_DEEP, GOLD, clamp(path * 1.4, 0.0, 1.0)) * path * (0.8 + 0.45 * facing * facing);
  c += GOLD_DEEP * near * (0.05 + 0.5 * glitter) * (0.7 + 0.5 * facing);
  return c;
}
`;
