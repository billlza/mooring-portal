// The near shore of the live scene (`shader.mjs`): the pier, the rocks and
// the lighthouse on them, and the light its turning lamp throws into the
// air. Everything here is measured from the lighthouse's axis: q is the
// place, with q.x counted from that axis.

export const SHORE = `
// A round wall seen from the side, at u across it (-1 its left edge, 1 its
// right): the light comes low from behind on the left, so only that edge
// is lit, and the rest stands in the sky's shade.
float rim(float u) {
  return 1.0 - smoothstep(-0.8, -0.15, u);
}

vec3 plaster(float u, vec2 q) {
  vec3 shade = mix(PLASTER, PLASTER_DARK, smoothstep(-0.2, 1.0, u));
  return mix(shade, PLASTER_LIT, rim(u)) * (0.96 + 0.05 * noise(q * 1.9) + 0.03 * noise(q * 0.31));
}

vec3 iron(float u) {
  return mix(IRON, IRON_LIT, 0.9 * rim(u));
}

// The pier's upper and lower edges at x: it comes toward the viewer as it
// runs to the right, so its top rises and its foot sinks.
float pierTop(float x) {
  return PIER_TOP - PIER_RISE * (x - PIER);
}

float pierFoot(float x) {
  return PIER_FOOT + PIER_SINK * (x - PIER);
}

vec3 pier(vec3 c, vec2 q) {
  if (q.x < PIER - 1.0 || q.y < 470.0) {
    return c;
  }
  float top = pierTop(q.x);
  float d = max(max(PIER - q.x, top - q.y), q.y - pierFoot(q.x));
  if (d > u_unit) {
    return c;
  }
  float below = q.y - top;
  // The lighthouse end is in the lamp's warmth; the rest falls dark.
  float warmth = 1.0 - smoothstep(30.0, 200.0, q.x);
  float block = floor((q.x - PIER) / JOINT);
  float tone = hash(ivec2(int(block), 3), 71u);
  vec3 wall = mix(WALL_DARK, WALL, 0.25 + 0.75 * warmth);
  wall = mix(wall, WALL_WARM, 0.8 * warmth * (1.0 - smoothstep(14.0, 62.0, below)));
  wall *= 0.86 + 0.2 * tone + 0.14 * (noise(q * vec2(0.08, 0.3)) - 0.5);
  // The joints between its blocks, and the ledge under its cap.
  float joint = abs(q.x - PIER - (block + 0.5) * JOINT) - 0.5 * JOINT;
  wall *= 1.0 - 0.45 * within(-joint - 0.9) * step(CAP, below);
  wall = mix(wall, STONE_LIT, 0.55 * (1.0 - smoothstep(0.0, 1.5, abs(below - CAP))) * (0.4 + 0.6 * warmth));
  // The top edge catches the sky.
  wall = mix(wall, STONE_LIT, 0.8 * (1.0 - smoothstep(0.0, 2.0, below)));
  wall *= 1.0 - 0.5 * smoothstep(STRAND - 26.0, STRAND, q.y);
  return mix(c, wall, within(d));
}

// A mooring post on the pier, standing at x.
vec3 bollard(vec3 c, vec2 q, float x) {
  vec2 r = vec2(q.x - x, q.y - pierTop(x));
  if (abs(r.x) > 11.0 || r.y < -25.0 || r.y > 2.0) {
    return c;
  }
  float stem = max(abs(r.x) - 4.6, max(-r.y - 17.0, r.y));
  float collar = max(abs(r.x) - 6.6, max(-r.y - 2.8, r.y));
  float cap = (length(vec2(r.x, r.y + 18.0) / vec2(8.8, 4.8)) - 1.0) * 4.8;
  float d = min(min(stem, collar), cap);
  return mix(c, iron(clamp(r.x / 8.8, -1.0, 1.0)) * (0.7 + 0.5 * step(r.y, -14.0)), within(d));
}

// The block the tower stands on.
vec3 plinth(vec3 c, vec2 q) {
  float d = max(max(PLINTH_LEFT - q.x, q.x - PLINTH_RIGHT), max(FOOT - q.y, q.y - PLINTH_FOOT));
  if (d > u_unit) {
    return c;
  }
  float below = q.y - FOOT;
  vec3 stone = mix(STONE, STONE_DARK, smoothstep(2.0, 26.0, below));
  stone = mix(stone, STONE_LIT, 1.0 - smoothstep(0.0, 2.2, below));
  return mix(c, stone * (0.92 + 0.16 * noise(q * 0.5)), within(d));
}

// Half the tower's width at height y: its shaft, the plinth band at its
// foot, and the flare that carries the gallery.
float girth(float y) {
  if (y >= NECK) {
    float shaft = mix(NECK_HALF, FOOT_HALF, (y - NECK) / (FOOT - NECK));
    return y >= BAND ? shaft + 2.0 : shaft;
  }
  float s = clamp((NECK - y) / (NECK - DECK), 0.0, 1.0);
  return NECK_HALF + (DECK_HALF - 1.5 - NECK_HALF) * s * s;
}

// A lit window, its middle at mid and its half size given.
vec3 window(vec3 c, vec2 q, vec2 mid, vec2 size) {
  vec2 r = abs(q - mid) - size;
  float pane = max(r.x, r.y);
  if (pane > 1.4 + u_unit) {
    return c;
  }
  c = mix(c, IRON, within(pane - 1.4));
  vec3 light = mix(PANE, GLASS_HOT, 0.45 * (1.0 - abs(q.x - mid.x) / size.x));
  return mix(c, light, within(pane));
}

vec3 tower(vec3 c, vec2 q) {
  if (abs(q.x) > DECK_HALF + 1.0 || q.y < DECK || q.y > FOOT + 1.0) {
    return c;
  }
  float breadth = girth(q.y);
  float d = max(abs(q.x) - breadth, max(DECK - q.y, q.y - FOOT));
  if (d > u_unit) {
    return c;
  }
  float u = clamp(q.x / breadth, -1.0, 1.0);
  vec3 wall = plaster(u, q);
  if (q.y < NECK) {
    // Under the gallery: its shadow, and the brackets that carry it.
    float s = (NECK - q.y) / (NECK - DECK);
    float bracket = abs(fract(asin(u) / 0.45 + 0.5) - 0.5);
    wall *= mix(0.92, 0.6, s) * (0.78 + 0.22 * smoothstep(0.08, 0.2, bracket));
  }
  // The band at its foot shows as a paler line.
  wall *= 1.0 + 0.22 * (1.0 - smoothstep(0.0, 1.3, abs(q.y - BAND)));
  c = mix(c, wall, within(d));
  c = window(c, q, vec2(18.5, 354.5), vec2(3.5, 8.5));
  c = window(c, q, vec2(21.5, 428.0), vec2(4.0, 9.0));
  // The door, in its frame, under a small gable.
  vec2 r = abs(q - vec2(22.0, 496.0)) - vec2(7.0, 17.0);
  float door = max(r.x, r.y);
  float gable = max(q.y - 479.0, (469.0 + abs(q.x - 22.0) - q.y) * 0.7071);
  c = mix(c, FRAME, within(min(door - 2.0, gable)));
  return mix(c, DOOR, within(door));
}

// The glass of the lantern room: the lamp's glare low in its middle, and
// the bright band of whichever beam is on the near side, crossing it as
// the lamp turns.
vec3 lantern(vec2 q, float angle) {
  float u = q.x / GLASS_HALF;
  vec2 from = vec2(q.x, q.y - LAMP) / vec2(8.0, 10.0);
  float glare = exp(-dot(from, from));
  float across = sin(angle) >= 0.0 ? cos(angle) : -cos(angle);
  float band = exp(-(u - across) * (u - across) / 0.09) * (0.35 + 0.65 * abs(sin(angle)));
  vec3 c = mix(GLASS_DIM, GLASS, clamp(1.0 - 0.8 * abs(u) + 0.3 * band, 0.0, 1.0));
  c = mix(c, GLASS_HOT, clamp(0.85 * glare + 0.8 * band, 0.0, 1.0));
  float bar = min(abs(abs(q.x) - 6.8), abs(abs(q.x) - 17.8)) - 0.55;
  float rail = abs(q.y - 257.0) - 0.45;
  return mix(c, IRON * 1.6, 0.85 * within(min(bar, rail)));
}

// The top of the lighthouse: gallery, lantern room, dome and spike.
vec3 head(vec3 c, vec2 q, float angle) {
  if (abs(q.x) > DECK_HALF + 1.0 || q.y < SPIKE - 1.0 || q.y > DECK + 1.0) {
    return c;
  }
  // The spike, the ball under it, and the dome.
  float spike = max(abs(q.x) - 0.7, max(SPIKE - q.y, q.y - BALL));
  float ball = min(length(vec2(q.x, q.y - BALL)) - BALL_R,
    max(abs(q.x) - 2.3, max(BALL - q.y, q.y - DOME - 2.0)));
  float rise = EAVES + 1.0 - DOME;
  float dome = max((length(vec2(q.x / DOME_HALF, (q.y - EAVES - 1.0) / rise)) - 1.0) * rise,
    q.y - EAVES - 1.0);
  float u = clamp(q.x / DOME_HALF, -1.0, 1.0);
  // The dome is dark metal, with the sky's gleam high on its lit side.
  vec2 gleam = vec2(q.x + 9.0, q.y - DOME - 7.0) / vec2(7.0, 4.0);
  vec3 roof = iron(u) + IRON_LIT * 0.55 * exp(-dot(gleam, gleam));
  c = mix(c, roof, within(min(min(spike, ball), dome)));
  // The eaves.
  float eaves = max(abs(q.x) - EAVES_HALF, max(EAVES - q.y, q.y - EAVES - 4.5));
  c = mix(c, iron(clamp(q.x / EAVES_HALF, -1.0, 1.0)) * 1.25, within(eaves));
  // The lantern room's glass.
  float glass = max(abs(q.x) - GLASS_HALF, max(GLASS_TOP - q.y, q.y - GLASS_FOOT));
  c = mix(c, lantern(q, angle), within(glass));
  // The room under it, dark, with the lamp's light down its lit side.
  float room = max(abs(q.x) - ROOM_HALF, max(GLASS_FOOT - q.y, q.y - DECK_TOP));
  float ru = clamp(q.x / ROOM_HALF, -1.0, 1.0);
  vec3 panel = mix(IRON, LAMPLIGHT, 0.85 * rim(ru))
    * (0.8 + 0.2 * smoothstep(0.03, 0.12, abs(fract(asin(ru) / 0.52 + 0.5) - 0.5)));
  c = mix(c, panel, within(room));
  // The gallery's deck, and the railing in front of the room.
  float deck = max(abs(q.x) - DECK_HALF, max(DECK_TOP - q.y, q.y - DECK));
  float du = clamp(q.x / DECK_HALF, -1.0, 1.0);
  c = mix(c, mix(IRON * 1.5, PLASTER_LIT, 0.8 * rim(du)), within(deck));
  if (abs(q.x) <= RAIL_HALF + 1.0 && q.y > RAIL - 2.0 && q.y < DECK_TOP) {
    float bulge = sqrt(max(RAIL_HALF * RAIL_HALF - q.x * q.x, 0.0));
    float post = abs(fract(asin(clamp(q.x / RAIL_HALF, -1.0, 1.0)) / 0.21) - 0.5) * 0.21 * bulge - 0.4;
    float bars = min(abs(q.y - RAIL) - 0.7, abs(q.y - 0.5 * (RAIL + DECK_TOP)) - 0.4);
    float railing = max(min(post, bars), max(abs(q.x) - RAIL_HALF, RAIL - 0.7 - q.y));
    c = mix(c, iron(clamp(q.x / RAIL_HALF, -1.0, 1.0)) * 0.9, 0.9 * within(railing));
  }
  return c;
}

// The rocks piled against the plinth and the pier's end. The pile is cut
// into boulders, one about each point of a broken lattice; a boulder is
// there if its point lies under the pile's line. Each is cut again into
// five faces about that point, and a face is lit by how far it turns up
// and toward the light.
const vec2 BOULDER = vec2(27.0, 19.0);

// Where the boulder of a lattice cell has its point, from the axis. The
// point keeps clear of its cell's edges, so that the nearest point to any
// place is in that place's cell or one beside it.
vec2 boulderAt(ivec2 cell) {
  return (vec2(cell) + 0.22 + 0.56 * vec2(hash(cell, 81u), hash(cell, 82u))) * BOULDER;
}

// How bright the face of a boulder is that looks along the angle a.
float faceLight(float a) {
  return smoothstep(-0.35, 0.95, dot(vec2(cos(a), sin(a)), vec2(-0.74, -0.67)));
}

// Whether the boulder with its point at a place is part of the pile.
bool piled(vec2 point) {
  return point.x >= -313.0 && point.x <= 77.0
    && point.y >= along(ROCKS, ROCKS_POINTS, point.x) + 5.0;
}

vec3 rocks(vec3 c, vec2 q) {
  if (q.x < -345.0 || q.x > 110.0 || q.y < 480.0 || q.y > STRAND + 4.0) {
    return c;
  }
  ivec2 cell = ivec2(floor(q / BOULDER));
  // The nearest boulder of the pile, the next nearest, and the nearest
  // point that has none.
  float best = 1e9;
  float next = 1e9;
  float bare = 1e9;
  ivec2 id = cell;
  vec2 from = vec2(0.0);
  for (int j = -1; j <= 1; j++) {
    for (int i = -1; i <= 1; i++) {
      ivec2 other = cell + ivec2(i, j);
      vec2 point = boulderAt(other);
      vec2 r = q - point;
      float d2 = dot(r, r);
      if (!piled(point)) {
        bare = min(bare, d2);
      } else if (d2 < best) {
        next = best;
        best = d2;
        id = other;
        from = r;
      } else if (d2 < next) {
        next = d2;
      }
    }
  }
  if (best > 1e8) {
    return c;
  }
  // How far the place is inside the pile's edge, and inside its own
  // boulder's: half the difference of its distances to two points.
  float near = sqrt(best);
  float inPile = 0.5 * (sqrt(bare) - near);
  float inBoulder = 0.5 * (sqrt(next) - near);
  float sector = TAU / 5.0;
  float turned = TAU * hash(id, 84u);
  float a = (atan(from.y, from.x) + turned) / sector;
  float k = floor(a);
  float part = a - k;
  float here = faceLight((k + 0.5) * sector - turned);
  float beside = faceLight((k + (part < 0.5 ? -0.5 : 1.5)) * sector - turned);
  float edge = min(part, 1.0 - part) * sector * near;
  float lit = mix(here, beside, 0.5 * clamp(1.0 - edge / u_unit, 0.0, 1.0));
  float tone = hash(id, 83u);
  // The lamp's side of the pile is the brighter.
  float mid = q.x - from.x;
  float warmth = mid < 0.0 ? mix(0.35, 1.0, smoothstep(-300.0, -40.0, mid)) : 0.55;
  vec3 stone = mix(ROCK_DARK, ROCK, 0.25 + 0.6 * tone);
  stone = mix(stone, ROCK_LIT, lit * warmth * (0.55 + 0.45 * tone));
  stone *= (0.3 + 0.7 * smoothstep(0.0, 1.5, inBoulder)) * (0.93 + 0.14 * noise(q * 0.8));
  stone *= 1.0 - 0.55 * smoothstep(STRAND - 18.0, STRAND, q.y);
  return mix(c, stone, within(-inPile) * within(q.y - STRAND - 1.0));
}

// The near shore, back to front, over whatever is behind it.
vec3 shore(vec3 c, vec2 q, float angle) {
  if (q.x < -322.0 || q.y < SPIKE - 1.0 || q.y > STRAND + 30.0) {
    return c;
  }
  c = pier(c, q);
  c = bollard(c, q, 95.0);
  c = bollard(c, q, 148.0);
  c = plinth(c, q);
  c = tower(c, q);
  c = head(c, q, angle);
  return rocks(c, q);
}

// The lamp's light in the air: a glow about the lantern that swells as a
// beam swings toward the viewer, and the two beams themselves, long while
// they cross the view and short as they turn toward it or away.
vec3 lamplight(vec3 c, vec2 q, float angle) {
  vec2 d = vec2(q.x, q.y - LAMP);
  float r = length(d);
  if (r > 760.0) {
    return c;
  }
  float facing = sin(angle) * sin(angle);
  float flash = facing * facing * facing;
  float glow = (0.42 + 0.8 * flash) * exp(-r / 15.0) + (0.07 + 0.3 * flash) * exp(-r / 64.0);
  float side = abs(cos(angle));
  float reach = abs(d.x) / max(side, 0.03);
  float wide = 4.5 + 0.045 * reach;
  float beam = exp(-d.y * d.y / (wide * wide)) * exp(-reach / 380.0) / (1.0 + reach / 60.0)
    * smoothstep(0.03, 0.35, side);
  return c + LAMPLIGHT * (glow + 1.5 * beam);
}
`;
