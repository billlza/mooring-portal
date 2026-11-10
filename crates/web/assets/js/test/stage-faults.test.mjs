// The live scene when things go wrong: too slow, a lost graphics card, a
// browser that will not play, and faults of the page's or the scene's own.
// In each the page must be left with the drawn image, and a fault of ours
// must be thrown. Run with
// `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { WARM_UP, WINDOW } from "../measure.mjs";
import { enliven } from "../stage.mjs";
import { running, settle, stand } from "./stand.mjs";

test("the watchdog lowers a slow scene, then gives it up for the drawn image", async () => {
  const page = await running();
  const widths = [page.canvas.width];
  for (let frame = 0; frame < 4 * (WARM_UP + WINDOW) && page.live; frame += 1) {
    page.tick(60);
    if (page.canvas.width !== widths.at(-1)) {
      widths.push(page.canvas.width);
    }
  }
  assert.deepEqual(widths, [1000, 750, 500]);
  // Each lowering drew the layers afresh, smaller, once it had settled.
  assert.deepEqual(page.sheets.map((sheet) => sheet.width), [1000, 475, 750, 357, 640, 256]);
  assert.equal(page.live, false);
  assert.equal(page.pause.hidden, true);
  assert.equal(page.waiting, 0);
  assert.equal(page.called("deleteProgram").length, 1);
  assert.equal(page.called("deleteTexture").length, 2);
  // Given up is given up.
  page.canvas.emit("webglcontextrestored");
  page.observers.resized();
  page.media.emit("change");
  await settle();
  assert.equal(page.waiting, 0);
  assert.equal(page.called("createProgram").length, 1);
});

test("a lost context shows the drawn image until it is restored", async () => {
  const page = await running();
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
  assert.equal(page.waiting, 0);
  page.gl.lost = false;
  page.canvas.emit("webglcontextrestored");
  assert.equal(page.called("createProgram").length, 2);
  assert.equal(page.sheets.length, 4);
  page.tick();
  assert.equal(page.live, true);
});

test("a context lost while the scene is being made waits for its return", async () => {
  // Lost before the layers arrive: nothing is made yet.
  const early = stand();
  early.gl.lost = true;
  await enliven(early.win);
  assert.equal(early.called("createProgram").length, 0);
  // Its return before the layers have arrived makes nothing either.
  const waiting = stand({ arrive: () => settle() });
  const made = enliven(waiting.win);
  waiting.canvas.emit("webglcontextrestored");
  assert.equal(waiting.called("createProgram").length, 0);
  await made;
  assert.equal(waiting.called("createProgram").length, 1);
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
  await enliven(midway.win);
  assert.equal(midway.waiting, 0);
  assert.equal(midway.called("deleteProgram").length, 0, "it went with the context");
  midway.gl.lost = false;
  midway.canvas.emit("webglcontextrestored");
  midway.tick();
  assert.equal(midway.live, true);
});

test("a browser that will not hand a drawn SVG to WebGL keeps the drawn image", async () => {
  const page = stand({
    during: {
      texImage2D() {
        throw new DOMException("the canvas is tainted", "SecurityError");
      },
    },
  });
  await enliven(page.win);
  assert.equal(page.live, false);
  assert.equal(page.waiting, 0);
  assert.equal(page.called("deleteProgram").length, 1, "what was made is given back");
  page.canvas.emit("webglcontextrestored");
  assert.equal(page.called("createProgram").length, 1);
});

test("a layer that does not arrive leaves the drawn image", async () => {
  const page = stand({ arrive: () => Promise.reject(new DOMException("no picture", "EncodingError")) });
  await enliven(page.win);
  assert.equal(page.live, false);
  assert.equal(page.called("createProgram").length, 0);
  page.media.emit("change");
  page.canvas.emit("webglcontextrestored");
  await settle();
  assert.equal(page.called("createProgram").length, 0);
});

test("a fault in the page or the scene is thrown, not hidden", async () => {
  for (const [options, message] of [
    [{ missing: ".hero" }, /no \.hero$/],
    [{ missing: ".hero-art" }, /no \.hero-art$/],
    [{ missing: "canvas.hero-scene" }, /no canvas\.hero-scene$/],
    [{ missing: ".hero-pause" }, /no \.hero-pause$/],
    [{ dataset: { pole: undefined } }, /no data-pole$/],
    [{ dataset: { shore: "800px" } }, /expected 1 numbers/],
    [{ dataset: { view: "1600" } }, /expected 2 numbers/],
  ]) {
    assert.throws(() => enliven(stand(options).win), message);
  }
  await assert.rejects(enliven(stand({ compiles: false }).win), /did not compile: a shader fault/);
  await assert.rejects(enliven(stand({ links: false }).win), /did not link: a linking fault/);
  await assert.rejects(enliven(stand({ pen: false }).win), /no 2D canvas/);
  const broken = stand({
    during: {
      createVertexArray() {
        throw new TypeError("a fault of our own");
      },
    },
  });
  await assert.rejects(enliven(broken.win), /a fault of our own/);
  assert.equal(broken.called("deleteProgram").length, 0);
  await assert.rejects(
    enliven(stand({ arrive: () => Promise.reject(new RangeError("something else")) }).win),
    /something else/,
  );
});

test("a first frame the graphics card refused is thrown, and the drawn image stays", async () => {
  const page = stand();
  await enliven(page.win);
  page.gl.error = 0x502;
  assert.throws(() => page.tick(), /first frame failed: WebGL error 0x502/);
  assert.equal(page.live, false);
  assert.equal(page.waiting, 0);
  assert.equal(page.called("deleteProgram").length, 1);
  // A lost context is told by its event, not taken for a refusal.
  const lost = stand();
  await enliven(lost.win);
  lost.gl.error = "CONTEXT_LOST_WEBGL";
  lost.tick();
  assert.equal(lost.live, true);
});
