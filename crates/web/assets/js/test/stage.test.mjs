// The live scene's ordinary life: when it starts, what it draws, and how
// it follows the page. Run with
// `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { enliven } from "../stage.mjs";
import { running, settle, stand } from "./stand.mjs";

test("a visitor who asked for less motion gets the drawn image and no downloads", async () => {
  const page = stand({ reduced: true });
  await enliven(page.win);
  assert.equal(page.live, false);
  assert.equal(page.pause.hidden, true);
  assert.deepEqual(page.canvas.asked, []);
  assert.deepEqual(page.loaded, []);
  assert.equal(page.waiting, 0);
  // Until they change their mind.
  page.media.matches = false;
  page.media.emit("change");
  await settle();
  page.tick();
  assert.equal(page.live, true);
});

test("a browser without WebGL 2 keeps the drawn image and downloads no layer", async () => {
  const page = stand({ webgl: false });
  await enliven(page.win);
  assert.deepEqual(page.canvas.asked, [["webgl2", {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    failIfMajorPerformanceCaveat: true,
  }]]);
  assert.deepEqual(page.loaded, []);
  assert.equal(page.waiting, 0);
  // It is not asked again.
  page.media.emit("change");
  page.canvas.emit("webglcontextrestored");
  await settle();
  assert.equal(page.canvas.asked.length, 1);
  assert.equal(page.live, false);
});

test("the scene goes live with its first frame, fitted to the picture", async () => {
  const page = stand();
  await enliven(page.win);
  assert.deepEqual(page.loaded.toSorted(), ["/assets/land.svg", "/assets/ring.svg"]);
  // Made and fitted, but not shown before a frame is painted.
  assert.equal(page.live, false);
  assert.equal(page.pause.hidden, true);
  assert.deepEqual([page.canvas.width, page.canvas.height], [1000, 600]);
  // The picture covers the box as the drawn image does: 0.625 pixels to
  // the unit, its top 40 units cut off.
  assert.deepEqual(page.told("u_origin"), [0, 40]);
  assert.deepEqual(page.told("u_unit"), [1.6]);
  assert.deepEqual(page.called("viewport").at(-1), ["viewport", 0, 0, 1000, 600]);
  assert.deepEqual(page.told("u_pole"), [1100, 500]);
  assert.deepEqual(page.told("u_lantern"), [1215, 768]);
  assert.deepEqual([page.told("u_span"), page.told("u_shore")], [[760], [800]]);
  // The layers are drawn at a texel to the pixel.
  assert.deepEqual(page.sheets, [
    { image: "/assets/land.svg", at: [0, 0], width: 1000, height: 625 },
    { image: "/assets/ring.svg", at: [0, 0], width: 475, height: 475 },
  ]);
  // Each goes to the unit the shader reads it from, with smaller copies
  // for the water to mirror, and keeps its colour multiplied by opacity.
  assert.deepEqual(page.uploads(), ["TEXTURE0", "TEXTURE1"]);
  assert.deepEqual([page.told("u_land"), page.told("u_ring")], [[0], [1]]);
  assert.equal(page.called("generateMipmap").length, 2);
  assert.equal(
    page.called("texParameteri").filter(([, , , value]) => value === "LINEAR_MIPMAP_LINEAR").length,
    2,
  );
  assert.deepEqual(page.called("pixelStorei"), [
    ["pixelStorei", "UNPACK_PREMULTIPLY_ALPHA_WEBGL", true],
  ]);
  // The copies made on the way are let go.
  assert.equal(page.scratch.length, 2);
  assert.ok(page.scratch.every((sheet) => sheet.width === 0 && sheet.height === 0));
  assert.equal(page.waiting, 1);
  page.tick();
  assert.equal(page.live, true);
  assert.equal(page.pause.hidden, false);
  assert.deepEqual(page.called("drawArrays"), [["drawArrays", "TRIANGLES", 0, 3]]);
  page.tick();
  assert.equal(page.called("drawArrays").length, 2);
});

test("frames closer together than a screen's are skipped, and time stops while away", async () => {
  const page = await running();
  page.tick(5);
  page.tick(5);
  assert.equal(page.called("drawArrays").length, 1);
  page.tick(5);
  assert.equal(page.called("drawArrays").length, 2);
  assert.ok(Math.abs(page.told("u_time")[0] - 0.015) < 1e-9);
  // After five seconds away, the sky has moved a tenth of a second.
  page.tick(5000);
  assert.ok(Math.abs(page.told("u_time")[0] - 0.115) < 1e-9);
});

test("the pause button stops and restarts the frames", async () => {
  const page = await running();
  page.tick();
  const paused = page.told("u_time")[0];
  page.pause.emit("click");
  assert.equal(page.pause.pressed, "true");
  assert.equal(page.waiting, 0);
  // The still frame stays where it is.
  assert.equal(page.live, true);
  page.tick(200);
  page.pause.emit("click");
  assert.equal(page.pause.pressed, "false");
  assert.equal(page.waiting, 1);
  page.tick();
  assert.equal(page.called("drawArrays").length, 3);
  // The scene takes up where it stopped: a pause takes no time.
  assert.equal(page.told("u_time")[0], paused);
});

test("a hidden page, or a hero scrolled away, draws nothing", async () => {
  const page = await running();
  page.document.hidden = true;
  page.document.emit("visibilitychange");
  assert.equal(page.waiting, 0);
  page.document.hidden = false;
  page.document.emit("visibilitychange");
  assert.equal(page.waiting, 1);
  // The last word on whether the hero is in view is the one that counts.
  page.observers.seen([{ isIntersecting: true }, { isIntersecting: false }]);
  assert.equal(page.waiting, 0);
  page.observers.seen([{ isIntersecting: true }]);
  assert.equal(page.waiting, 1);
});

test("less motion, asked for while the scene runs, shows the drawn image", async () => {
  const page = await running();
  page.media.matches = true;
  page.media.emit("change");
  assert.equal(page.live, false);
  assert.equal(page.pause.hidden, true);
  assert.equal(page.waiting, 0);
  // A change of box then is left for later.
  page.canvas.clientWidth = 800;
  page.observers.resized();
  assert.equal(page.canvas.width, 1000);
  page.media.matches = false;
  page.media.emit("change");
  page.tick();
  assert.equal(page.live, true);
  assert.equal(page.canvas.width, 800);
});

test("a new box is fitted just before the next frame, never between frames", async () => {
  const page = await running();
  page.canvas.clientWidth = 800;
  page.canvas.clientHeight = 500;
  page.observers.resized();
  // Sizing the buffer clears it, so it waits for the frame that repaints.
  assert.equal(page.canvas.width, 1000);
  const before = page.gl.calls.length;
  page.tick();
  assert.deepEqual([page.canvas.width, page.canvas.height], [800, 500]);
  const since = page.gl.calls.slice(before).map(([name]) => name);
  assert.ok(since.indexOf("viewport") !== -1 && since.indexOf("viewport") < since.indexOf("drawArrays"));
  // The layers are drawn afresh once the box has held still.
  assert.equal(page.sheets.length, 2);
  page.tick(200);
  assert.equal(page.sheets.length, 2);
  page.tick(200);
  assert.deepEqual(page.sheets.slice(2).map((sheet) => sheet.width), [800, 380]);
});

test("a paused scene is fitted and repainted at once when its box changes", async () => {
  const page = await running();
  page.pause.emit("click");
  page.canvas.clientWidth = 500;
  page.canvas.clientHeight = 300;
  page.observers.resized();
  assert.deepEqual([page.canvas.width, page.canvas.height], [500, 300]);
  assert.equal(page.called("drawArrays").length, 2);
  assert.equal(page.waiting, 0);
  // A box that is gone is left alone.
  page.canvas.clientWidth = 0;
  page.observers.resized();
  assert.equal(page.canvas.width, 500);
  assert.equal(page.called("drawArrays").length, 2);
});

test("a canvas with no box yet is painted only once it has one", async () => {
  const page = stand({ width: 0 });
  await enliven(page.win);
  assert.deepEqual(page.sheets, []);
  page.tick();
  page.tick();
  assert.equal(page.called("drawArrays").length, 0);
  assert.equal(page.live, false);
  page.canvas.clientWidth = 1000;
  page.observers.resized();
  page.tick();
  // Fitted, dressed and painted in the one frame: never a frame without
  // its layers.
  assert.equal(page.sheets.length, 2);
  assert.equal(page.called("drawArrays").length, 1);
  assert.equal(page.live, true);
});

test("a denser screen, and a small graphics card, are followed", async () => {
  const page = await running({ largest: 512 });
  assert.deepEqual(page.sheets.map((sheet) => [sheet.width, sheet.height]), [[512, 320], [475, 475]]);
  page.win.devicePixelRatio = 2;
  page.tick();
  assert.deepEqual([page.canvas.width, page.canvas.height], [2000, 1200]);
  assert.deepEqual(page.told("u_unit"), [0.8]);
});

test("the mouse slides the sky, a finger does not", async () => {
  const page = await running();
  const slide = () => page.told("u_drift");
  page.win.emit("pointermove", { pointerType: "touch", clientX: 1000, clientY: 600 });
  for (let frame = 0; frame < 120; frame += 1) {
    page.tick();
  }
  // Only the slow sway of the sky itself.
  const sway = () => {
    const seconds = page.told("u_time")[0];
    return [5 * Math.sin(seconds * 0.07), 2 * Math.sin(seconds * 0.05 + 1)];
  };
  assert.deepEqual(slide(), sway());
  // The far right and the top: the sky slides its full reach that way.
  page.win.emit("pointermove", { pointerType: "mouse", clientX: 1000, clientY: 0 });
  page.tick();
  assert.ok(slide()[0] - sway()[0] > 0.1 && slide()[0] - sway()[0] < 1, "it eases, it does not jump");
  for (let frame = 0; frame < 300; frame += 1) {
    page.tick();
  }
  assert.ok(Math.abs(slide()[0] - sway()[0] - 8) < 0.05, String(slide()[0] - sway()[0]));
  assert.ok(Math.abs(slide()[1] - sway()[1] + 3) < 0.05, String(slide()[1] - sway()[1]));
});
