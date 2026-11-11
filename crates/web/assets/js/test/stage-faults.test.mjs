// The live scene when things go wrong: too slow, a lost graphics card, a
// frame the card refuses, and faults of the page's or the scene's own. In
// each the page must be left with the drawn image, and a fault of ours
// must be thrown. Run with
// `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { WARM_UP, WINDOW } from "../measure.mjs";
import { enliven } from "../stage.mjs";
import { running, stand } from "./stand.mjs";

test("the watchdog lowers a slow scene, then gives it up for the drawn image", () => {
  const page = running();
  const widths = [page.canvas.width];
  for (let frame = 0; frame < 4 * (WARM_UP + WINDOW) && page.live; frame += 1) {
    page.tick(60);
    if (page.canvas.width !== widths.at(-1)) {
      widths.push(page.canvas.width);
    }
  }
  assert.deepEqual(widths, [1000, 750, 500]);
  assert.equal(page.live, false);
  assert.equal(page.why, "too-slow");
  assert.equal(page.pause.hidden, true);
  assert.equal(page.waiting, 0);
  // What the scene held is given back.
  assert.equal(page.called("deleteProgram").length, 1);
  assert.equal(page.called("deleteVertexArray").length, 1);
  // Given up is given up.
  page.canvas.emit("webglcontextrestored");
  page.observers.resized();
  page.media.emit("change");
  assert.equal(page.waiting, 0);
  assert.equal(page.called("createProgram").length, 1);
});

test("a paused scene is not judged for speed, however slowly its frames come", () => {
  const page = running();
  page.pause.emit("click");
  for (let step = 0; step < 2 * (WARM_UP + WINDOW); step += 1) {
    page.canvas.clientWidth = 900 + (step % 2);
    page.observers.resized();
    page.tick(60);
  }
  assert.equal(page.canvas.width, page.canvas.clientWidth);
  assert.equal(page.live, true);
});

test("a lost context shows the drawn image until it is restored", () => {
  const page = running();
  const event = {
    prevented: false,
    preventDefault() {
      this.prevented = true;
    },
  };
  page.gl.lost = true;
  page.canvas.emit("webglcontextlost", event);
  assert.equal(event.prevented, true);
  assert.equal(page.live, false);
  assert.equal(page.why, "context-lost");
  assert.equal(page.waiting, 0);
  page.gl.lost = false;
  page.canvas.emit("webglcontextrestored");
  assert.equal(page.called("createProgram").length, 2);
  page.tick();
  assert.equal(page.live, true);
  // A scene made again is asked about again.
  assert.equal(page.called("getError").length, 2);
});

test("a context lost while the scene is being made waits for its return", () => {
  // Lost before anything is made: nothing is.
  const early = stand();
  early.gl.lost = true;
  enliven(early.win);
  assert.equal(early.called("createProgram").length, 0);
  assert.equal(early.waiting, 0);
  early.gl.lost = false;
  early.canvas.emit("webglcontextrestored");
  early.tick();
  assert.equal(early.live, true);
  // Lost half-way through the making: the fault is the loss, not ours.
  let losses = 1;
  const midway = stand({
    during: {
      createVertexArray(gl) {
        if (losses > 0) {
          losses -= 1;
          gl.lost = true;
          throw new TypeError("the context is lost");
        }
      },
    },
  });
  enliven(midway.win);
  assert.equal(midway.waiting, 0);
  assert.equal(midway.called("deleteProgram").length, 0, "it went with the context");
  midway.gl.lost = false;
  midway.canvas.emit("webglcontextrestored");
  midway.tick();
  assert.equal(midway.live, true);
  // Lost while its shaders compile: a lost context answers nothing, which
  // is not a shader that will not compile, nor a program that will not
  // link.
  let drops = 1;
  const compiling = stand({
    during: {
      compileShader(gl) {
        if (drops > 0) {
          drops -= 1;
          gl.lost = true;
        }
      },
    },
  });
  enliven(compiling.win);
  compiling.tick();
  assert.equal(compiling.live, false);
  assert.equal(compiling.why, undefined);
  compiling.canvas.emit("webglcontextlost", { preventDefault() {} });
  assert.equal(compiling.waiting, 0);
  compiling.gl.lost = false;
  compiling.canvas.emit("webglcontextrestored");
  compiling.tick();
  assert.equal(compiling.live, true);
});

test("a fault in the page or the scene is thrown, not hidden", () => {
  for (const [options, message] of [
    [{ missing: ".hero" }, /no \.hero$/],
    [{ missing: ".hero-art" }, /no \.hero-art$/],
    [{ missing: "canvas.hero-scene" }, /no canvas\.hero-scene$/],
    [{ missing: ".hero-pause" }, /no \.hero-pause$/],
    [{ dataset: { view: undefined } }, /no data-view$/],
    [{ dataset: { view: "1600" } }, /expected 2 numbers/],
    [{ dataset: { view: "1600px 1000" } }, /expected 2 numbers/],
    [{ compiles: false }, /did not compile: a shader fault/],
    [{ links: false }, /did not link: a linking fault/],
  ]) {
    assert.throws(() => enliven(stand(options).win), message);
  }
  // A fault while the scene is being made: nothing was made to give back.
  const broken = stand({
    during: {
      createVertexArray() {
        throw new TypeError("a fault of our own");
      },
    },
  });
  assert.throws(() => enliven(broken.win), /a fault of our own/);
  assert.equal(broken.called("deleteProgram").length, 0);
  assert.equal(broken.waiting, 0);
  // A fault once it is made, here a stylesheet that places the picture in
  // something other than percentages: what was made is given back.
  const misplaced = stand({ position: "left bottom" });
  assert.throws(() => enliven(misplaced.win), /object-position is not two percentages/);
  assert.equal(misplaced.called("deleteProgram").length, 1);
  assert.equal(misplaced.called("deleteVertexArray").length, 1);
  assert.equal(misplaced.waiting, 0);
  assert.equal(misplaced.live, false);
});

test("a frame the graphics card refused is thrown, and the drawn image stays", () => {
  const page = stand();
  enliven(page.win);
  page.gl.error = 0x502;
  assert.throws(() => page.tick(), /the graphics card refused the scene: WebGL error 0x502/);
  assert.equal(page.live, false);
  assert.equal(page.why, "refused");
  assert.equal(page.waiting, 0);
  assert.equal(page.called("deleteProgram").length, 1);
  // So is one refused later, when the scene is made again on a context
  // that came back.
  const later = running();
  later.gl.lost = true;
  later.canvas.emit("webglcontextlost", { preventDefault() {} });
  later.gl.lost = false;
  later.canvas.emit("webglcontextrestored");
  later.gl.error = 0x505;
  assert.throws(() => later.tick(), /WebGL error 0x505/);
  assert.equal(later.live, false);
  assert.equal(later.why, "refused");
  assert.equal(later.pause.hidden, true);
  assert.equal(later.waiting, 0);
  // And a scene that has been refused is not made a third time.
  later.canvas.emit("webglcontextrestored");
  assert.equal(later.called("createProgram").length, 2);
});

test("a frame on a context that has just been lost shows nothing", () => {
  const page = stand();
  enliven(page.win);
  // Lost between the making and the first frame; its event has yet to come.
  page.gl.lost = true;
  page.gl.error = "CONTEXT_LOST_WEBGL";
  page.tick();
  assert.equal(page.live, false);
  assert.equal(page.pause.hidden, true);
  // The loss is told by its event; it is not taken for a refusal.
  assert.equal(page.called("deleteProgram").length, 0);
  page.canvas.emit("webglcontextlost", { preventDefault() {} });
  assert.equal(page.waiting, 0);
  page.gl.lost = false;
  page.canvas.emit("webglcontextrestored");
  page.tick();
  assert.equal(page.live, true);
});
