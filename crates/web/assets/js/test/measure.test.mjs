// The scene's arithmetic. Run with
// `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  FLOOR, LIMIT, WARM_UP, WINDOW, anchor, buffer, cover, numbers, watch, watching,
} from "../measure.mjs";

const PICTURE = { width: 1600, height: 1000 };

test("numbers reads exactly the count asked for", () => {
  assert.deepEqual(numbers("1100 500 280", 3), [1100, 500, 280]);
  assert.deepEqual(numbers("  -3.5   0.25 ", 2), [-3.5, 0.25]);
  for (const [text, count] of [
    ["1100 500", 3],
    ["1100 500 280 9", 3],
    ["", 1],
    ["1e3", 1],
    ["12px", 1],
    ["1100,500", 2],
    ["NaN", 1],
    ["Infinity", 1],
    ["0x10", 1],
  ]) {
    assert.throws(() => numbers(text, count), /numbers/, `${JSON.stringify(text)} as ${count}`);
  }
});

test("anchor reads object-position as fractions", () => {
  assert.deepEqual(anchor("62% 100%"), [0.62, 1]);
  assert.deepEqual(anchor("0% 50%"), [0, 0.5]);
  assert.deepEqual(anchor("70.5% 100%"), [0.705, 1]);
  for (const text of ["62%", "left bottom", "10px 20px", "62% 100% 0%", "", "62 100"]) {
    assert.throws(() => anchor(text), /object-position/, JSON.stringify(text));
  }
});

test("cover matches object-fit: cover", () => {
  // The same shape: the whole picture, whatever the anchor.
  assert.deepEqual(cover({ width: 800, height: 500 }, PICTURE, [0.62, 1]), {
    scale: 0.5,
    left: 0,
    top: 0,
  });
  // A taller box: the height fits and the sides are cut, 62% of the cut on
  // the left.
  const tall = cover({ width: 500, height: 1000 }, PICTURE, [0.62, 1]);
  assert.equal(tall.scale, 1);
  assert.equal(tall.top, 0);
  assert.ok(Math.abs(tall.left - 1100 * 0.62) < 1e-9, String(tall.left));
  // A wider box: the width fits and the top is cut, all of it (100%).
  const wide = cover({ width: 3200, height: 1000 }, PICTURE, [0.62, 1]);
  assert.equal(wide.scale, 2);
  assert.equal(wide.left, 0);
  assert.equal(wide.top, 500);
  // Anchored at the top instead, nothing is cut there.
  assert.equal(cover({ width: 3200, height: 1000 }, PICTURE, [0.62, 0]).top, 0);
});

test("cover puts each corner of the box inside the picture", () => {
  for (const box of [
    { width: 375, height: 812 },
    { width: 1440, height: 900 },
    { width: 2560, height: 600 },
  ]) {
    for (const at of [[0, 0], [0.62, 1], [1, 1]]) {
      const { scale, left, top } = cover(box, PICTURE, at);
      const right = left + box.width / scale;
      const bottom = top + box.height / scale;
      assert.ok(left >= -1e-9 && top >= -1e-9, `${JSON.stringify(box)} ${at}`);
      assert.ok(right <= PICTURE.width + 1e-9 && bottom <= PICTURE.height + 1e-9);
    }
  }
});

test("buffer follows the screen's density up to its limits", () => {
  const box = { width: 1000, height: 500 };
  assert.deepEqual(buffer(box, 1, 1), { width: 1000, height: 500, scale: 1 });
  assert.deepEqual(buffer(box, 1.5, 1), { width: 1500, height: 750, scale: 1.5 });
  // Density above the limit is not followed.
  assert.equal(LIMIT.ratio, 2);
  assert.deepEqual(buffer(box, 3, 1), { width: 2000, height: 1000, scale: 2 });
  // The watchdog's quality scales both sides.
  assert.deepEqual(buffer(box, 1, 0.5), { width: 500, height: 250, scale: 0.5 });
  // Exactly the budget is allowed.
  const full = buffer({ width: LIMIT.pixels / 1000, height: 1000 }, 1, 1);
  assert.equal(full.width * full.height, LIMIT.pixels);
  // No more pixels than the budget, in the box's shape.
  const big = buffer({ width: 3000, height: 1500 }, 2, 1);
  assert.ok(big.width * big.height <= LIMIT.pixels, `${big.width}x${big.height}`);
  assert.ok(big.width * big.height > LIMIT.pixels * 0.98);
  assert.ok(Math.abs(big.width / big.height - 2) < 0.01);
  // A box with no area still gets a buffer.
  assert.deepEqual(buffer({ width: 0, height: 0 }, 2, 1), { width: 1, height: 1, scale: 2 });
});

/** The state after `count` frames of `ms` each. */
function run(state, count, ms) {
  let verdict = "keep";
  for (let frame = 0; frame < count; frame += 1) {
    ({ state, verdict } = watch(state, ms));
    if (verdict !== "keep") {
      return { state, verdict, frame };
    }
  }
  return { state, verdict, frame: count };
}

test("the watchdog leaves a smooth scene alone", () => {
  const { state, verdict } = run(watching(), 2000, 16.7);
  assert.equal(verdict, "keep");
  assert.equal(state.quality, 1);
});

test("the watchdog lowers a slow scene step by step, then stops it", () => {
  let state = watching();
  const qualities = [];
  let stopped = false;
  for (let round = 0; round < 10 && !stopped; round += 1) {
    const result = run(state, 10_000, 45);
    state = result.state;
    if (result.verdict === "stop") {
      stopped = true;
      break;
    }
    assert.equal(result.verdict, "lower");
    // Each verdict comes after the warm-up and one full window.
    assert.equal(result.frame, WARM_UP + WINDOW - 1);
    qualities.push(state.quality);
  }
  assert.ok(stopped, "a scene too slow at its lowest quality is stopped");
  assert.deepEqual(qualities, [0.75, FLOOR]);
  for (let index = 1; index < qualities.length; index += 1) {
    assert.ok(qualities[index] < qualities[index - 1]);
  }
  assert.equal(qualities.at(-1), FLOOR);
});

test("the watchdog judges the average, and forgives a pause", () => {
  // At the limit on average, or under it: kept.
  assert.equal(run(watching(), 3000, LIMIT.frameMs - 0.5).verdict, "keep");
  assert.equal(run(watching(), 3000, LIMIT.frameMs).verdict, "keep");
  // A browser saving power draws thirty frames a second; that is enough.
  assert.equal(run(watching(), 3000, 1000 / 30).verdict, "keep");
  // Just over: lowered.
  assert.equal(run(watching(), 3000, LIMIT.frameMs + 0.5).verdict, "lower");
  // The longest gap that still counts as a frame is a slow frame.
  assert.equal(run(watching(), 3000, LIMIT.gapMs).verdict, "lower");
  assert.equal(run(watching(), 3000, LIMIT.gapMs + 1).verdict, "keep");
  // A long gap is the tab having been away, not a slow frame.
  let state = watching();
  for (let frame = 0; frame < 3000; frame += 1) {
    const ms = frame % 10 === 0 ? 5000 : 16.7;
    const result = watch(state, ms);
    assert.equal(result.verdict, "keep", `frame ${frame}`);
    state = result.state;
  }
});

test("a lowered scene warms up again before it is judged", () => {
  const first = run(watching(), 10_000, 45);
  assert.equal(first.verdict, "lower");
  // Slow frames during the new warm-up are not held against it.
  const warm = run(first.state, WARM_UP, 100);
  assert.equal(warm.verdict, "keep");
  assert.equal(run(warm.state, 2000, 16.7).verdict, "keep");
});
