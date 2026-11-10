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
  assert.equal(page.why, "too-slow");
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

test("a paused scene is not judged for speed, however slowly its frames come", async () => {
  const page = await running();
  page.pause.emit("click");
  for (let step = 0; step < 2 * (WARM_UP + WINDOW); step += 1) {
    page.canvas.clientWidth = 900 + (step % 2);
    page.observers.resized();
    page.tick(60);
  }
  assert.equal(page.canvas.width, page.canvas.clientWidth);
  assert.equal(page.live, true);
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
  assert.equal(page.why, "context-lost");
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
  assert.equal(page.why, "layers-barred");
  assert.equal(page.waiting, 0);
  assert.equal(page.called("deleteProgram").length, 1, "what was made is given back");
  page.canvas.emit("webglcontextrestored");
  assert.equal(page.called("createProgram").length, 1);
});

test("a layer that does not arrive leaves the drawn image", async () => {
  // Either layer, whichever comes first.
  for (const missing of [0, 1]) {
    let asked = 0;
    const page = stand({
      arrive() {
        asked += 1;
        return asked - 1 === missing ? Promise.reject(new Error("no picture")) : Promise.resolve();
      },
    });
    await enliven(page.win);
    assert.equal(page.loaded.length, 2);
    assert.equal(page.live, false);
    assert.equal(page.why, "no-layers");
    assert.equal(page.called("createProgram").length, 0);
    page.media.emit("change");
    page.canvas.emit("webglcontextrestored");
    await settle();
    assert.equal(page.called("createProgram").length, 0);
  }
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
});

test("only the browser's own refusal of a drawn SVG is taken for a browser that will not play", async () => {
  // A layer the graphics card is not given for another reason, or with
  // another kind of error that happens to carry the name, is a fault.
  for (const other of [
    new DOMException("another refusal", "InvalidStateError"),
    Object.assign(new Error("another refusal"), { name: "SecurityError" }),
  ]) {
    const page = stand({
      during: {
        texImage2D() {
          throw other;
        },
      },
    });
    await assert.rejects(enliven(page.win), /another refusal/);
    assert.equal(page.called("deleteProgram").length, 1, "what was made is given back");
    assert.equal(page.live, false);
    assert.equal(page.why, undefined);
  }
});

test("a frame the graphics card refused is thrown, and the drawn image stays", async () => {
  const page = stand();
  await enliven(page.win);
  page.gl.error = 0x502;
  assert.throws(() => page.tick(), /the graphics card refused the scene: WebGL error 0x502/);
  assert.equal(page.live, false);
  assert.equal(page.why, "refused");
  assert.equal(page.waiting, 0);
  assert.equal(page.called("deleteProgram").length, 1);
  // So is one refused later, when the layers are sent again for a new box.
  const later = await running();
  later.canvas.clientWidth = 800;
  later.observers.resized();
  later.tick();
  later.tick(200);
  later.gl.error = 0x505;
  assert.throws(() => later.tick(200), /WebGL error 0x505/);
  assert.equal(later.live, false);
  assert.equal(later.pause.hidden, true);
  assert.equal(later.waiting, 0);
});

test("a frame on a context that has just been lost shows nothing", async () => {
  const page = stand();
  await enliven(page.win);
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
