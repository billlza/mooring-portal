// The live scene's two shaders. The picture is worked out here, pixel by
// pixel, with no image behind it: a night sky whose stars twinkle, a far
// shore, a lighthouse on its rocks whose lamp turns, and a sea that moves
// and glitters under it. The drawn image the page shows first
// (`assets/hero.rs`) is the same picture standing still. Where things
// stand and what colour they are is therefore written twice, there and in
// `HEAD` below, under the same names; a test there fails when one of the
// two is changed without the other.
//
// Positions are in the picture's own units, y downwards, as in the SVG.
// The sky is in `shader-sky.mjs`, the lighthouse and its shore in
// `shader-shore.mjs`, and the sea in `shader-water.mjs`.

import { SHORE } from "./shader-shore.mjs";
import { SKY } from "./shader-sky.mjs";
import { WATER } from "./shader-water.mjs";

/** One triangle that covers the view, without any buffer. */
export const VERTEX = `#version 300 es
void main() {
  vec2 corner = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
}
`;

const HEAD = `#version 300 es
precision highp float;
// The hash needs all 32 bits; a fragment shader's integers default to less.
precision highp int;

uniform vec2 u_size;    // the drawing buffer, in pixels
uniform vec2 u_origin;  // where in the picture its top-left pixel falls
uniform float u_unit;   // picture units per pixel
uniform float u_time;   // seconds the scene has run
uniform vec2 u_drift;   // how far the sky has slid behind the shore

out vec4 colour;

#define rgb(r, g, b) (vec3(r, g, b) / 255.0)

const float TAU = 6.28318530718;

// Where things stand. The lighthouse is measured from its axis.
const float TALL = 790.0;
const float AXIS = 2576.0;
const float SEA = 545.0;
const float STRAND = 597.0;
const float FOOT = 513.0;
const float FOOT_HALF = 46.0;
const float BAND = 487.0;
const float NECK = 317.0;
const float NECK_HALF = 30.0;
const float DECK = 303.0;
const float DECK_TOP = 298.0;
const float DECK_HALF = 41.5;
const float RAIL = 277.0;
const float RAIL_HALF = 40.0;
const float ROOM_HALF = 21.5;
const float GLASS_FOOT = 274.0;
const float GLASS_TOP = 244.0;
const float GLASS_HALF = 22.0;
const float EAVES = 240.0;
const float EAVES_HALF = 26.5;
const float DOME = 224.0;
const float DOME_HALF = 25.0;
const float BALL = 219.5;
const float BALL_R = 4.5;
const float SPIKE = 188.0;
const float LAMP = 259.0;
const float PLINTH_LEFT = -58.0;
const float PLINTH_RIGHT = 12.0;
const float PLINTH_FOOT = 541.0;
const float PIER = 9.0;
const float PIER_TOP = 527.0;
const float PIER_RISE = 0.0717;
const float PIER_FOOT = 592.0;
const float PIER_SINK = 0.035;
const float CAP = 11.0;
const float JOINT = 47.0;

// The far range, the nearer hill, the rocks' crest and the farthest range:
// each a line through these points, the rocks' from the axis and the
// others across the picture.
const int FAR = 0;
const int FAR_POINTS = 17;
const int NEAR = 17;
const int NEAR_POINTS = 12;
const int ROCKS = 29;
const int ROCKS_POINTS = 18;
const int REMOTE = 47;
const int REMOTE_POINTS = 11;
const vec2 LINE[58] = vec2[58](
  vec2(0.0, 549.0), vec2(560.0, 549.0), vec2(760.0, 538.0), vec2(980.0, 533.0),
  vec2(1220.0, 539.0), vec2(1420.0, 549.0), vec2(1795.0, 549.0), vec2(1875.0, 526.0),
  vec2(1955.0, 506.0), vec2(2035.0, 492.0), vec2(2155.0, 478.0), vec2(2265.0, 461.0),
  vec2(2355.0, 471.0), vec2(2475.0, 488.0), vec2(2595.0, 500.0), vec2(2843.0, 512.0),
  vec2(3160.0, 520.0),
  vec2(0.0, 549.0), vec2(2275.0, 549.0), vec2(2315.0, 523.0), vec2(2395.0, 505.0),
  vec2(2465.0, 490.0), vec2(2535.0, 478.0), vec2(2622.0, 473.0), vec2(2695.0, 456.0),
  vec2(2785.0, 442.0), vec2(2875.0, 446.0), vec2(2995.0, 470.0), vec2(3160.0, 498.0),
  vec2(-313.0, 598.0), vec2(-297.0, 590.0), vec2(-271.0, 582.0), vec2(-236.0, 570.0),
  vec2(-206.0, 556.0), vec2(-181.0, 547.0), vec2(-151.0, 537.0), vec2(-131.0, 530.0),
  vec2(-106.0, 521.0), vec2(-89.0, 515.0), vec2(-73.0, 514.0), vec2(-58.0, 519.0),
  vec2(-46.0, 541.0), vec2(9.0, 543.0), vec2(24.0, 556.0), vec2(44.0, 568.0),
  vec2(64.0, 584.0), vec2(77.0, 598.0),
  vec2(0.0, 549.0), vec2(1180.0, 549.0), vec2(1290.0, 540.0), vec2(1400.0, 531.0),
  vec2(1510.0, 524.0), vec2(1620.0, 519.0), vec2(1730.0, 514.0), vec2(1840.0, 509.0),
  vec2(1950.0, 503.0), vec2(2300.0, 505.0), vec2(3160.0, 530.0)
);

// The colours, each named for what it paints.
const vec3 SKY_TOP = rgb(5.0, 19.0, 32.0);
const vec3 SKY_MID = rgb(12.0, 30.0, 47.0);
const vec3 SKY_LOW = rgb(21.0, 41.0, 61.0);
const vec3 SKY_RIM = rgb(34.0, 53.0, 73.0);
const vec3 HAZE = rgb(75.0, 84.0, 99.0);
const vec3 CLOUD = rgb(58.0, 77.0, 102.0);
const vec3 STAR_COOL = rgb(190.0, 214.0, 255.0);
const vec3 STAR_WARM = rgb(255.0, 232.0, 200.0);
const vec3 REMOTE_LAND = rgb(26.0, 44.0, 63.0);
const vec3 FAR_LAND = rgb(22.0, 38.0, 56.0);
const vec3 NEAR_LAND = rgb(16.0, 30.0, 46.0);
const vec3 MIST = rgb(44.0, 58.0, 76.0);
const vec3 TOWN_WARM = rgb(255.0, 214.0, 150.0);
const vec3 TOWN_PALE = rgb(255.0, 236.0, 200.0);
const vec3 SEA_FAR = rgb(22.0, 41.0, 60.0);
const vec3 SEA_MID = rgb(13.0, 30.0, 46.0);
const vec3 SEA_DEEP = rgb(6.0, 19.0, 33.0);
const vec3 SEA_SHEEN = rgb(121.0, 147.0, 176.0);
const vec3 SEA_SHADE = rgb(7.0, 12.0, 18.0);
const vec3 GOLD = rgb(251.0, 226.0, 178.0);
const vec3 GOLD_DEEP = rgb(214.0, 150.0, 96.0);
const vec3 PLASTER_LIT = rgb(248.0, 224.0, 197.0);
const vec3 PLASTER = rgb(66.0, 81.0, 103.0);
const vec3 PLASTER_DARK = rgb(48.0, 57.0, 72.0);
const vec3 IRON = rgb(24.0, 28.0, 36.0);
const vec3 IRON_LIT = rgb(106.0, 93.0, 82.0);
const vec3 GLASS = rgb(219.0, 182.0, 133.0);
const vec3 GLASS_DIM = rgb(164.0, 117.0, 77.0);
const vec3 GLASS_HOT = rgb(253.0, 250.0, 236.0);
const vec3 LAMPLIGHT = rgb(255.0, 196.0, 122.0);
const vec3 PANE = rgb(232.0, 197.0, 140.0);
const vec3 DOOR = rgb(19.0, 30.0, 46.0);
const vec3 FRAME = rgb(126.0, 136.0, 152.0);
const vec3 STONE = rgb(55.0, 66.0, 78.0);
const vec3 STONE_LIT = rgb(108.0, 116.0, 128.0);
const vec3 STONE_DARK = rgb(30.0, 36.0, 45.0);
const vec3 WALL = rgb(46.0, 44.0, 47.0);
const vec3 WALL_WARM = rgb(97.0, 80.0, 70.0);
const vec3 WALL_DARK = rgb(16.0, 25.0, 34.0);
const vec3 ROCK = rgb(51.0, 48.0, 50.0);
const vec3 ROCK_LIT = rgb(141.0, 132.0, 120.0);
const vec3 ROCK_DARK = rgb(21.0, 23.0, 28.0);

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

// Noise in four sizes, each half the last: about [0, 1].
float cloudy(vec2 p) {
  return (noise(p) + 0.5 * noise(p * 2.03 + 17.0) + 0.25 * noise(p * 4.01 + 31.0)
    + 0.125 * noise(p * 8.05 + 47.0)) / 1.875;
}

// The sine of an angle that grows with the time. The angle is brought
// within one turn first: a graphics card may lose its sine's accuracy far
// from zero, and the scene may run for hours.
float wave(float angle) {
  return sin(TAU * fract(angle / TAU));
}

// How much of a pixel lies on the near side of an edge that is d picture
// units away: all of it where d is well below nothing.
float within(float d) {
  return clamp(0.5 - d / u_unit, 0.0, 1.0);
}

// The height at x of the line LINE[from .. from + count): straight from
// point to point, eased a little at each. Beyond its ends it is level.
float along(int from, int count, float x) {
  float y = LINE[from].y;
  for (int i = from; i < from + count - 1; i++) {
    vec2 a = LINE[i];
    vec2 b = LINE[i + 1];
    if (x >= a.x) {
      float t = min((x - a.x) / (b.x - a.x), 1.0);
      y = mix(a.y, b.y, mix(t, t * t * (3.0 - 2.0 * t), 0.5));
    }
  }
  return y;
}

// Smooth noise along a line, in [0, 1]: noise in one dimension.
float noise1(float x, uint salt) {
  float corner = floor(x);
  float f = x - corner;
  ivec2 cell = ivec2(int(corner), 0);
  return mix(hash(cell, salt), hash(cell + ivec2(1, 0), salt), f * f * (3.0 - 2.0 * f));
}

// The hills and saddles along a crest at x, within [-1, 1]: noise in five
// sizes from 150 picture units down, each half as tall as the last.
float rough(float x, uint salt) {
  float sum = 0.0;
  float tall = 0.5;
  float at = x / 150.0;
  for (uint i = 0u; i < 5u; i++) {
    sum += tall * (2.0 * noise1(at, salt + i) - 1.0);
    tall *= 0.5;
    at = at * 2.03 + 0.37;
  }
  return sum;
}

// The ragged edge of the woods along a crest at x, within [-1.05, 1.05]:
// thicker along some stretches than others.
float woods(float x, uint salt) {
  float edge = noise1(x / 3.0, salt + 5u) - 0.5 + 0.5 * (noise1(x / 1.3, salt + 6u) - 0.5);
  return edge * (0.4 + noise1(x / 70.0, salt + 7u));
}

// A range's crest at x, where its line is at level: hills and saddles
// as tall as the range stands above the water, and its woods' edge.
float crest(float level, float x, uint salt) {
  float high = max(SEA + 4.0 - level, 0.0);
  float wooded = min(high, 8.0) / 8.0;
  return level - (0.16 * high + 1.2 * wooded) * rough(x, salt) - 1.1 * wooded * woods(x, salt);
}

// The most a crest may climb above its line where the line is at level.
float climb(float level) {
  float high = max(SEA + 4.0 - level, 0.0);
  return 0.16 * high + 2.4 * min(high, 8.0) / 8.0;
}

// The lamp turns once in eighteen seconds, and throws two beams, so one
// sweeps past every nine.
float turn() {
  return TAU * fract(u_time / 18.0);
}
`;

const MAIN = `
void main() {
  vec2 p = u_origin + vec2(gl_FragCoord.x, u_size.y - gl_FragCoord.y) * u_unit;
  vec2 q = vec2(p.x - AXIS, p.y);
  float angle = turn();
  // The far waterline: sky and shore above it, the sea below. Each is
  // worked out only where it shows, and called for once, so that it is
  // compiled once.
  float wet = within(SEA - p.y);
  vec3 above = wet < 1.0 ? heaven(p) : vec3(0.0);
  vec3 below = wet > 0.0 ? sea(p, angle) : vec3(0.0);
  vec3 c = mix(above, below, wet);
  c = shore(c, q, angle);
  c = lamplight(c, q, angle);
  // A grain of noise, so the dark gradients do not band.
  c += (hash(ivec2(gl_FragCoord.xy), 31u) - 0.5) / 255.0;
  colour = vec4(c, 1.0);
}
`;

export const FRAGMENT = HEAD + SKY + SHORE + WATER + MAIN;
