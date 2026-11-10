// The live scene's two shaders. The picture is the drawn hero image set
// in motion: the sky turns about the pole, stars twinkle, the lantern
// flickers, and the harbour mirrors all of it in moving water. The land and
// the ring of star trails are the drawn image's own layers, so the scene
// and the image it is painted over cannot drift apart. The sky, the water
// and the lantern's light are painted here, in the image's own colours:
// every colour named below is one the image uses (`assets.rs` checks).
//
// Positions are in the picture's own units, y downwards, as in the SVG.

/** One triangle that covers the view, without any buffer. */
export const VERTEX = `#version 300 es
void main() {
  vec2 corner = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
}
`;

export const FRAGMENT = `#version 300 es
precision highp float;
// The hash needs all 32 bits; a fragment shader's integers default to less.
precision highp int;

uniform vec2 u_size;       // the drawing buffer, in pixels
uniform vec2 u_origin;     // where in the picture its top-left pixel falls
uniform float u_unit;      // picture units per pixel
uniform float u_time;      // seconds the scene has run
uniform vec2 u_drift;      // how far the sky has slid behind the land
uniform vec2 u_picture;    // the picture's width and height
uniform vec2 u_pole;       // the pole the sky turns about
uniform float u_span;      // the side of the square the ring layer covers
uniform float u_shore;     // the waterline
uniform vec2 u_lantern;    // the lantern at the masthead
uniform sampler2D u_land;  // ridges, shores, jetty and boat
uniform sampler2D u_ring;  // the ring of star trails

out vec4 colour;

const float TAU = 6.28318530718;
// One turn of the sky in about nine minutes: slow enough to be calm, fast
// enough to see.
const float TURN = 0.0116;

// PCG (O'Neill 2014), the RXS-M-XS output for one 32-bit word.
uint pcg(uint v) {
  uint state = v * 747796405u + 2891336453u;
  uint word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
  return (word >> 22u) ^ word;
}

// A number in [0, 1) for a lattice cell; the salt picks which.
float hash(ivec2 cell, uint salt) {
  uvec2 at = uvec2(cell + 32768);
  uint mixed = pcg(at.x + pcg(at.y + salt * 2654435761u));
  return float(mixed >> 8u) * (1.0 / 16777216.0);
}

// Smooth noise in [0, 1].
float noise(vec2 p) {
  vec2 corner = floor(p);
  vec2 f = p - corner;
  vec2 ease = f * f * (3.0 - 2.0 * f);
  ivec2 cell = ivec2(corner);
  float a = hash(cell, 11u);
  float b = hash(cell + ivec2(1, 0), 11u);
  float c = hash(cell + ivec2(0, 1), 11u);
  float d = hash(cell + ivec2(1, 1), 11u);
  return mix(mix(a, b, ease.x), mix(c, d, ease.x), ease.y);
}

// A vector turned by an angle, clockwise on the screen as SVG turns.
vec2 turned(vec2 v, float angle) {
  float c = cos(angle);
  float s = sin(angle);
  return vec2(v.x * c - v.y * s, v.x * s + v.y * c);
}

vec3 rgb(float r, float g, float b) {
  return vec3(r, g, b) / 255.0;
}

// The sky's gradient and the band of the Milky Way.
vec3 sky(vec2 p, vec2 slid) {
  float t = clamp(p.y / u_picture.y, 0.0, 1.0);
  vec3 c = t < 0.5
    ? mix(rgb(1.0, 3.0, 11.0), rgb(5.0, 13.0, 42.0), t / 0.5)
    : mix(rgb(5.0, 13.0, 42.0), rgb(13.0, 36.0, 89.0), min((t - 0.5) / 0.3, 1.0));
  vec2 band = turned(slid - vec2(820.0, 300.0), radians(16.0)) / vec2(900.0, 130.0);
  return mix(c, rgb(201.0, 217.0, 255.0), 0.13 * max(1.0 - length(band), 0.0));
}

// The stars, fixed to a sky that turns about the pole; s is the place in
// that sky.
vec3 stars(vec3 c, vec2 s) {
  const float CELL = 54.0;
  ivec2 cell = ivec2(floor(s / CELL));
  if (hash(cell, 1u) > 0.62) {
    return c;
  }
  vec2 at = (vec2(cell) + 0.14 + 0.72 * vec2(hash(cell, 2u), hash(cell, 3u))) * CELL;
  float size = hash(cell, 4u);
  float radius = 0.35 + 1.3 * size * size * size;
  float tint = hash(cell, 5u);
  vec3 light = tint < 0.12 ? rgb(207.0, 224.0, 255.0)
    : tint < 0.2 ? rgb(255.0, 231.0, 194.0)
    : vec3(1.0);
  float twinkle = 0.68 + 0.32 * sin(u_time * mix(0.5, 2.3, hash(cell, 6u)) + TAU * hash(cell, 7u));
  // A star smaller than a pixel is drawn a pixel wide and dimmer, so that
  // it neither vanishes nor flashes as the sky carries it across pixels.
  float drawn = max(radius, 0.6 * u_unit);
  float dim = max(radius / drawn, 0.55);
  float d = length(s - at);
  float disc = 1.0 - smoothstep(drawn - 0.7 * u_unit, drawn + 0.7 * u_unit, d);
  float bright = mix(0.45, 1.0, hash(cell, 8u)) * twinkle * dim;
  // One star in sixteen has a soft halo.
  float halo = hash(cell, 9u) < 0.06 ? 0.16 * exp(-d * d / 26.0) : 0.0;
  return mix(c, light, clamp(disc * bright + halo * twinkle, 0.0, 1.0));
}

// The ring of star trails, turning with the sky.
vec3 ring(vec3 c, vec2 s, float blur) {
  vec2 at = s / u_span + 0.5;
  if (any(lessThan(at, vec2(0.0))) || any(greaterThan(at, vec2(1.0)))) {
    return c;
  }
  vec4 layer = textureLod(u_ring, at, blur);
  return layer.rgb + c * (1.0 - layer.a);
}

// Now and then, a shooting star.
vec3 shooting(vec3 c, vec2 p) {
  const float EVERY = 8.0;
  float window = floor(u_time / EVERY);
  ivec2 id = ivec2(int(mod(window, 30000.0)), 3);
  if (hash(id, 21u) > 0.5) {
    return c;
  }
  float age = (u_time - window * EVERY - 1.0 - 5.0 * hash(id, 22u)) / 0.9;
  if (age < 0.0 || age > 1.0) {
    return c;
  }
  vec2 from = vec2(mix(60.0, 980.0, hash(id, 23u)), mix(30.0, 260.0, hash(id, 24u)));
  vec2 along = normalize(vec2(1.0, mix(0.22, 0.55, hash(id, 25u))));
  vec2 behind = from + along * 380.0 * age - p;
  float back = dot(behind, along);
  float aside = length(behind - along * back);
  const float TAIL = 170.0;
  if (back < -2.0 || back > TAIL) {
    return c;
  }
  float width = max(1.1, u_unit);
  float trail = (1.0 - max(back, 0.0) / TAIL) * exp(-aside * aside / (2.0 * width * width));
  return c + rgb(220.0, 234.0, 255.0) * trail * sin(3.14159265 * age);
}

// The lantern: a warm glow that is never quite still.
vec3 lantern(vec3 c, vec2 p) {
  float waver = 0.5 * sin(u_time * 6.1) + 0.3 * sin(u_time * 9.7 + 1.7) + 0.2 * sin(u_time * 17.3 + 0.4);
  float d = length(p - u_lantern);
  float r = d / (36.0 * (1.0 + 0.04 * waver));
  if (r < 1.0) {
    vec3 light = mix(rgb(255.0, 210.0, 122.0), rgb(255.0, 179.0, 71.0), min(r / 0.35, 1.0));
    float alpha = r < 0.35 ? mix(0.9, 0.32, r / 0.35) : mix(0.32, 0.0, (r - 0.35) / 0.65);
    c = mix(c, light, alpha * (1.0 + 0.08 * waver));
  }
  float core = 1.0 - smoothstep(2.4 - u_unit, 2.4 + u_unit, d);
  return mix(c, rgb(255.0, 241.0, 200.0), core);
}

vec4 land(vec2 p, float blur) {
  return textureLod(u_land, p / u_picture, blur);
}

// Everything above the waterline. Seen directly it is sharp. The water
// mirrors it softened, by blur levels of the layers' smaller copies, and
// without the stars, which no ripple would hold.
vec3 above(vec2 p, float blur) {
  vec2 slid = p - u_drift;
  vec2 s = turned(slid - u_pole, u_time * TURN);
  vec3 c = sky(p, slid);
  if (blur == 0.0) {
    c = stars(c, s);
    c = shooting(c, p);
  }
  c = ring(c, s, blur);
  vec4 ground = land(p, blur);
  c = ground.rgb + c * (1.0 - ground.a);
  return lantern(c, p);
}

// The harbour: dark water, the scene mirrored in its ripples, and the
// glints where a ripple catches a light.
vec3 water(vec2 p) {
  float depth = clamp((p.y - u_shore) / (u_picture.y - u_shore), 0.0, 1.0);
  vec3 c = mix(rgb(10.0, 26.0, 69.0), rgb(1.0, 4.0, 12.0), depth);
  float t = u_time;
  // How long a ripple is on the screen: short by the far shore, long near
  // the viewer, and never under a few pixels, so the water does not shimmer.
  float ripple = max(3.0, 3.0 * u_unit) + 24.0 * depth;
  // Where the point is, counted in ripples: rows from the far shore, and
  // across from the middle.
  float row = 8.0 * log(ripple);
  float across = (p.x - 0.5 * u_picture.x) / ripple;
  float calm = smoothstep(0.0, 0.06, depth);
  // Each row of water slides what it mirrors to one side, and no two rows
  // agree.
  float sway = 0.6 * sin(row * 4.3 + t * 1.05 + across * 0.12)
    + 0.3 * sin(row * 9.7 - t * 1.6 + across * 0.35)
    + 0.4 * (noise(vec2(across * 0.16 + t * 0.05, row * 1.4 - t * 0.2)) - 0.5);
  float lift = sin(row * 6.1 + t * 0.8 + across * 0.5);
  vec2 mirrored = vec2(
    p.x + sway * ripple * 0.42 * calm,
    min(2.0 * u_shore - p.y + lift * ripple * 0.16 * calm, u_shore - 0.5));
  vec3 image = above(mirrored, 1.5 + 1.3 * depth);
  float light = max(image.r, max(image.g, image.b));
  c = mix(c, image, mix(0.5, 0.22, depth));
  c += image * smoothstep(0.45, 1.0, light) * 0.3;
  // Glints: thin dashes that lie along the rows and come and go, brightest
  // where the water mirrors a light.
  float band = ripple * 1.04;
  float line = floor(row);
  float off = abs(fract(row) - 0.5) * band;
  float thin = 1.0 - smoothstep(0.35, min(0.16 * band, 1.3), off);
  ivec2 id = ivec2(int(line), 7);
  float along = across * 0.2 + t * mix(-0.2, 0.2, hash(id, 51u)) + 37.0 * hash(id, 52u);
  float dashes = smoothstep(0.6, 0.82, noise(vec2(along, line * 0.37 + t * 0.07)));
  float glint = thin * dashes * calm;
  float fromRing = (p.x - u_pole.x) / 300.0;
  float nearRing = exp(-fromRing * fromRing);
  c += (image * 1.6 * smoothstep(0.2, 0.7, light) + rgb(159.0, 208.0, 255.0) * 0.16 * nearRing)
    * glint * mix(1.0, 0.6, depth);
  // The lantern's path across the water, broken by the same ripples.
  float aside = (p.x - u_lantern.x + sway * ripple * 0.3) / (4.0 + 15.0 * depth);
  float path = exp(-aside * aside) * exp(-(p.y - u_shore) / 58.0) * smoothstep(0.0, 0.03, depth);
  c += rgb(255.0, 196.0, 107.0) * path * (0.2 + 0.8 * glint + 0.25 * (0.5 + 0.5 * lift));
  return c;
}

void main() {
  vec2 p = u_origin + vec2(gl_FragCoord.x, u_size.y - gl_FragCoord.y) * u_unit;
  vec3 c;
  if (p.y < u_shore) {
    c = above(p, 0.0);
  } else {
    c = water(p);
    vec4 ground = land(p, 0.0);
    c = ground.rgb + c * (1.0 - ground.a);
    c = lantern(c, p);
  }
  // A grain of noise, so the dark gradients do not band.
  c += (hash(ivec2(gl_FragCoord.xy), 31u) - 0.5) / 255.0;
  colour = vec4(c, 1.0);
}
`;
