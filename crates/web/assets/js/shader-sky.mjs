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

// The far range and the nearer hill, standing dark against the sky's rim.
vec3 hills(vec3 c, vec2 p) {
  if (p.y < 425.0) {
    return c;
  }
  float far = farCrest(p.x);
  vec3 range = mix(FAR_HIGH, FAR_DEEP, clamp((p.y - far) / 60.0, 0.0, 1.0));
  c = mix(c, range, within(far - p.y));
  float near = nearCrest(p.x);
  vec3 hill = mix(NEAR_PALE, NEAR_DARK, smoothstep(AXIS - 60.0, AXIS + 130.0, p.x));
  hill = mix(hill, NEAR_DARK, 0.5 * clamp((p.y - near) / 70.0, 0.0, 1.0));
  return mix(c, hill, within(near - p.y));
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
  c = hills(c, p);
  if (p.y > SEA - 30.0) {
    c += lamps(p, 1.0);
  }
  return c;
}
`;
