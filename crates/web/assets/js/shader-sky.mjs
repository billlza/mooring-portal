// The sky of the live scene (`shader.mjs`): its light, its stars, its
// thin cloud, and the far shore under it with a town's lamps at the water.

export const SKY = `
// How much of the low haze about the lighthouse lies over a place.
float veil(vec2 p) {
  vec2 from = (p - vec2(AXIS, 500.0)) / vec2(383.0, 180.0);
  return 0.95 * exp(-dot(from, from));
}

// The sky's own colour: darkest overhead, and paler in the haze.
vec3 sky(vec2 p, float haze) {
  float t = clamp(p.y / SEA, 0.0, 1.0);
  vec3 c = t < 0.52 ? mix(SKY_TOP, SKY_MID, t / 0.52)
    : t < 0.7 ? mix(SKY_MID, SKY_LOW, (t - 0.52) / 0.18)
    : mix(SKY_LOW, SKY_RIM, (t - 0.7) / 0.3);
  return mix(c, HAZE, haze);
}

// Thin cloud: long streaks to the right of the sky, drifting slowly.
float wisps(vec2 s) {
  float band = smoothstep(215.0, 300.0, s.y) * (1.0 - smoothstep(420.0, 500.0, s.y));
  float side = smoothstep(AXIS - 640.0, AXIS - 150.0, s.x);
  if (band * side <= 0.0) {
    return 0.0;
  }
  vec2 at = vec2((s.x + u_time * 1.4) / 310.0, s.y / 40.0);
  return smoothstep(0.5, 0.8, cloudy(at)) * band * side;
}

// The stars, one to a cell of the sky where there is one; clear says how
// much of a star shows through the haze and the cloud.
vec3 stars(vec3 c, vec2 s, float clear) {
  const float CELL = 46.0;
  ivec2 cell = ivec2(floor(s / CELL));
  if (hash(cell, 1u) > 0.7) {
    return c;
  }
  vec2 at = (vec2(cell) + 0.1 + 0.8 * vec2(hash(cell, 2u), hash(cell, 3u))) * CELL;
  float size = hash(cell, 4u);
  float radius = 0.3 + 1.0 * size * size * size;
  float tint = hash(cell, 5u);
  vec3 light = tint < 0.14 ? STAR_COOL : tint < 0.24 ? STAR_WARM : vec3(1.0);
  // Each star has its own pace, and now and then a sharper glint.
  float pace = mix(0.7, 2.9, hash(cell, 6u));
  float phase = TAU * hash(cell, 7u);
  float twinkle = 0.6 + 0.4 * wave(u_time * pace + phase)
    + 0.35 * smoothstep(0.82, 1.0, wave(u_time * pace * 0.37 + phase * 3.0));
  // A star smaller than a pixel is drawn a pixel wide and dimmer, so that
  // it neither vanishes nor flashes as the sky carries it across pixels.
  float drawn = max(radius, 0.6 * u_unit);
  float dim = max(radius / drawn, 0.55);
  float d = length(s - at);
  float disc = 1.0 - smoothstep(drawn - 0.7 * u_unit, drawn + 0.7 * u_unit, d);
  float bright = mix(0.4, 1.0, hash(cell, 8u)) * twinkle * dim;
  // One star in twelve has a soft halo.
  float halo = hash(cell, 9u) < 0.08 ? 0.2 * exp(-d * d / 22.0) : 0.0;
  return mix(c, light, clamp(disc * bright + halo * twinkle, 0.0, 1.0) * clear);
}

// The faint stars between them: too many and too small to draw one by one
// in the still image, so only the live sky has them.
vec3 dust(vec3 c, vec2 s, float clear) {
  const float CELL = 17.0;
  ivec2 cell = ivec2(floor(s / CELL));
  if (hash(cell, 61u) > 0.55) {
    return c;
  }
  vec2 at = (vec2(cell) + 0.1 + 0.8 * vec2(hash(cell, 62u), hash(cell, 63u))) * CELL;
  float drawn = max(0.32, 0.6 * u_unit);
  float d = length(s - at);
  float disc = 1.0 - smoothstep(drawn - 0.7 * u_unit, drawn + 0.7 * u_unit, d);
  float twinkle = 0.55 + 0.45 * wave(u_time * mix(1.1, 3.4, hash(cell, 64u)) + TAU * hash(cell, 65u));
  float bright = mix(0.12, 0.5, hash(cell, 66u)) * twinkle * max(0.32 / drawn, 0.5);
  return mix(c, vec3(1.0), disc * bright * clear);
}

// Now and then, a shooting star, away from the lighthouse.
vec3 shooting(vec3 c, vec2 p, float clear) {
  const float EVERY = 9.0;
  float window = floor(u_time / EVERY);
  ivec2 id = ivec2(int(mod(window, 30000.0)), 3);
  if (hash(id, 21u) > 0.45) {
    return c;
  }
  float age = (u_time - window * EVERY - 1.0 - 5.0 * hash(id, 22u)) / 0.9;
  if (age < 0.0 || age > 1.0) {
    return c;
  }
  vec2 from = vec2(AXIS - mix(1500.0, 420.0, hash(id, 23u)), mix(40.0, 230.0, hash(id, 24u)));
  vec2 dir = normalize(vec2(1.0, mix(0.22, 0.55, hash(id, 25u))));
  vec2 behind = from + dir * 330.0 * age - p;
  float back = dot(behind, dir);
  float aside = length(behind - dir * back);
  const float TAIL = 150.0;
  if (back < -2.0 || back > TAIL) {
    return c;
  }
  float width = max(1.0, u_unit);
  float trail = (1.0 - max(back, 0.0) / TAIL) * exp(-aside * aside / (2.0 * width * width));
  return c + STAR_COOL * trail * sin(3.14159265 * age) * clear;
}

// A range of the far shore: where its line is in LINE, the salt of its
// roughness, the colour of its land, how plainly the lie of its slopes
// shows, and how high above the water the mist at its foot reaches. The
// farther off a range stands, the paler its land, the fainter its slopes
// and the lower its mist.
struct Range {
  int from;
  int count;
  uint salt;
  vec3 land;
  float relief;
  float mist;
};

const Range REMOTE_RANGE = Range(REMOTE, REMOTE_POINTS, 141u, REMOTE_LAND, 0.2, 35.0);
const Range FAR_RANGE = Range(FAR, FAR_POINTS, 121u, FAR_LAND, 0.6, 60.0);
const Range NEAR_RANGE = Range(NEAR, NEAR_POINTS, 131u, NEAR_LAND, 1.0, 60.0);

// Range r's crest at x.
float ridge(Range r, float x) {
  return crest(along(r.from, r.count, x), x, r.salt);
}

// The highest range r's crest may stand at x.
float highest(Range r, float x) {
  float level = along(r.from, r.count, x);
  return level - climb(level);
}

// The lie of a range's slopes about at: noise in three sizes, within
// [0, 1.75].
float slopes(vec2 at) {
  return noise(at) + 0.5 * noise(at * 2.07 + 13.0) + 0.25 * noise(at * 4.13 + 29.0);
}

// How a range's land is lit at p, below its crest at top: the rises that
// run down from the crest lit on the lighthouse's side, the hollows
// between them darker, and the grain of the woods over all. The rises
// lean by lean as the land falls away.
float relief(vec2 p, float top, float lean, float offset) {
  float d = max(p.y - top, 0.0);
  vec2 at = vec2((p.x + lean * d) / 20.0, d / 30.0) + offset;
  at.x += 1.6 * (noise(at * vec2(0.35, 0.6) + 3.1) - 0.5);
  float h = slopes(at);
  float east = slopes(at + vec2(0.3, 0.0));
  float side = clamp((p.x - AXIS) / 80.0, -1.0, 1.0);
  return 0.1 * side * (east - h) / 0.3 + 0.18 * (h - 0.875) + 0.12 * (noise(p / 2.4 + offset * 7.0) - 0.5);
}

// Range r over c at p, its crest above p at top: its land, lit by the lie
// of its slopes where it is seen (the water mirrors too little of it to
// show them), and paler in the mist at its foot and in the haze about the
// lighthouse.
vec3 range(vec3 c, vec2 p, Range r, float top, bool seen) {
  float cover = within(top - p.y);
  if (cover <= 0.0) {
    return c;
  }
  float mist = smoothstep(SEA - r.mist, SEA + 2.0, p.y);
  vec3 land = r.land;
  if (seen) {
    // The land falls away from the crest: down to the left where the crest
    // climbs to the right.
    float slope = (along(r.from, r.count, p.x + 25.0) - along(r.from, r.count, p.x - 25.0)) / 50.0;
    land *= 1.0 + r.relief * (1.0 - mist) * relief(p, top, clamp(-1.6 * slope, -0.9, 0.9), float(r.salt));
  }
  land = mix(land, MIST, 0.7 * mist);
  return mix(c, mix(land, HAZE, 0.4 * veil(p)), cover);
}

// The ranges of the far shore over c at p, from the farthest. A range is
// worked out only where it may show: not where no crest can climb to
// cover any of the pixel, nor where a nearer range covers all of it.
vec3 hills(vec3 c, vec2 p, bool seen) {
  if (p.y < 420.0) {
    return c;
  }
  float skyline = min(min(highest(REMOTE_RANGE, p.x), highest(FAR_RANGE, p.x)), highest(NEAR_RANGE, p.x));
  if (within(skyline - p.y) <= 0.0) {
    return c;
  }
  float near = ridge(NEAR_RANGE, p.x);
  if (within(near - p.y) < 1.0) {
    float far = ridge(FAR_RANGE, p.x);
    if (within(far - p.y) < 1.0) {
      c = range(c, p, REMOTE_RANGE, ridge(REMOTE_RANGE, p.x), seen);
    }
    c = range(c, p, FAR_RANGE, far, seen);
  }
  return range(c, p, NEAR_RANGE, near, seen);
}

// How likely a lamp is at x along the far shore: a town under the far
// range's highest part, and a few houses along the coast before it.
float settled(float x) {
  float town = smoothstep(2170.0, 2200.0, x) * (1.0 - smoothstep(2330.0, 2362.0, x));
  float coast = smoothstep(1930.0, 1960.0, x) * (1.0 - smoothstep(2120.0, 2170.0, x));
  return 0.8 * town + 0.16 * coast;
}

// The town's lamps, at p and its two neighbours' cells along the shore.
// Each is stretched down the page by a factor, for its path on the water.
vec3 lamps(vec2 p, float stretch) {
  const float CELL = 6.0;
  vec3 light = vec3(0.0);
  int here = int(floor(p.x / CELL));
  for (int k = -1; k <= 1; k++) {
    ivec2 cell = ivec2(here + k, 5);
    float x = (float(here + k) + hash(cell, 41u)) * CELL;
    if (hash(cell, 42u) > settled(x)) {
      continue;
    }
    float up = hash(cell, 43u);
    vec2 from = (p - vec2(x, SEA - 2.5 - 15.0 * up * up)) / vec2(1.0, stretch);
    float d2 = dot(from, from);
    float glow = exp(-d2 / 1.3) + 0.22 * exp(-d2 / 12.0);
    float flicker = 0.86 + 0.14 * wave(u_time * mix(0.6, 2.4, hash(cell, 44u)) + TAU * hash(cell, 45u));
    light += mix(TOWN_WARM, TOWN_PALE, hash(cell, 46u)) * glow * flicker * mix(0.35, 1.0, hash(cell, 47u));
  }
  return light;
}

// Everything above the far waterline.
vec3 heaven(vec2 p) {
  // The sky slides a little behind the shore as the pointer moves.
  vec2 s = p - u_drift;
  float haze = veil(p);
  float cloud = wisps(s);
  vec3 c = sky(p, haze);
  float clear = (1.0 - haze) * (1.0 - 0.85 * cloud) * (1.0 - 0.75 * smoothstep(SEA - 110.0, SEA - 10.0, p.y));
  c = dust(c, s, clear);
  c = stars(c, s, clear);
  c = shooting(c, p, clear);
  c = mix(c, CLOUD, 0.5 * cloud);
  c = hills(c, p, true);
  if (p.y > SEA - 30.0) {
    c += lamps(p, 1.0);
  }
  return c;
}
`;
