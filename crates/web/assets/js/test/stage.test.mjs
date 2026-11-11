// The live scene's ordinary life: when it starts, what it draws, and how
// it follows the page. Run with
// `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { enliven } from "../stage.mjs";
import { running, stand } from "./stand.mjs";

test("a visitor who asked for less motion gets the drawn image, and no scene is made", () => {
  const page = stand({ reduced: true });
  enliven(page.win);
  assert.equal(page.live, false);
  assert.equal(page.why, "reduced-motion");
  assert.equal(page.pause.hidden, true);
  assert.deepEqual(page.canvas.asked, []);
  assert.equal(page.waiting, 0);
  // Until they change their mind.
  page.media.matches = false;
  page.media.emit("change");
  page.tick();
  assert.equal(page.live, true);
  assert.equal(page.why, "live");
});

test("a browser without WebGL 2 keeps the drawn image", () => {
  const page = stand({ webgl: false });
  enliven(page.win);
  assert.deepEqual(page.canvas.asked, [["webgl2", {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    failIfMajorPerformanceCaveat: true,
  }]]);
  assert.equal(page.waiting, 0);
  assert.equal(page.why, "no-webgl2");
  // It is not asked again.
  page.media.emit("change");
  page.canvas.emit("webglcontextrestored");
  assert.equal(page.canvas.asked.length, 1);
  assert.equal(page.live, false);
});

test("the scene goes live with its first frame, fitted to the picture", () => {
  const page = stand();
  enliven(page.win);
  // Made and fitted at once, but not shown before a frame is painted;
  // until then the hero says nothing of its scene.
  assert.equal(page.called("createProgram").length, 1);
  assert.equal(page.live, false);
  assert.equal(page.why, undefined);
  assert.equal(page.pause.hidden, true);
  assert.deepEqual([page.canvas.width, page.canvas.height], [1000, 600]);
  // The picture covers the box as the drawn image does: 0.625 pixels to
  // the unit, its top 40 units cut off.
  assert.deepEqual(page.told("u_origin"), [0, 40]);
  assert.deepEqual(page.told("u_unit"), [1.6]);
  assert.deepEqual(page.told("u_size"), [1000, 600]);
  assert.deepEqual(page.called("viewport").at(-1), ["viewport", 0, 0, 1000, 600]);
  // It is one triangle drawn by one program, with nothing blended.
  assert.deepEqual(page.called("disable"), [["disable", "DEPTH_TEST"], ["disable", "BLEND"]]);
  assert.equal(page.called("bindVertexArray").length, 1);
  assert.equal(page.waiting, 1);
  page.tick();
  assert.equal(page.live, true);
  assert.equal(page.why, "live");
  assert.equal(page.pause.hidden, false);
  assert.deepEqual(page.called("drawArrays"), [["drawArrays", "TRIANGLES", 0, 3]]);
  // All the shader is told is its view, the time, and how far the sky has
  // slid: the picture itself is the shader's own.
  assert.deepEqual(page.uniforms(), ["u_drift", "u_origin", "u_size", "u_time", "u_unit"]);
  page.tick();
  assert.equal(page.called("drawArrays").length, 2);
  // The graphics card is asked what it refused once the scene has been
  // painted, and not again with every frame: the answer is waited for.
  assert.equal(page.called("getError").length, 1);
});

test("frames closer together than a screen's are skipped, and time stops while away", () => {
  const page = running();
  page.tick(5);
  page.tick(5);
  assert.equal(page.called("drawArrays").length, 1);
  page.tick(5);
  assert.equal(page.called("drawArrays").length, 2);
  assert.ok(Math.abs(page.told("u_time")[0] - 0.015) < 1e-9);
  // After five seconds away, the scene has moved a tenth of a second.
  page.tick(5000);
  assert.ok(Math.abs(page.told("u_time")[0] - 0.115) < 1e-9);
});

test("the pause button stops and restarts the frames", () => {
  const page = running();
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

test("a hidden page, or a hero scrolled away, draws nothing", () => {
  const page = running();
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

test("less motion, asked for while the scene runs, shows the drawn image", () => {
  const page = running();
  page.media.matches = true;
  page.media.emit("change");
  assert.equal(page.live, false);
  assert.equal(page.why, "reduced-motion");
  assert.equal(page.pause.hidden, true);
  assert.equal(page.waiting, 0);
  // A change of box then is left for later: no frame is asked for.
  page.canvas.clientWidth = 800;
  page.observers.resized();
  assert.equal(page.waiting, 0);
  assert.equal(page.canvas.width, 1000);
  assert.equal(page.live, false);
  page.media.matches = false;
  page.media.emit("change");
  page.tick();
  assert.equal(page.live, true);
  assert.equal(page.canvas.width, 800);
  // The scene was not made a second time for it.
  assert.equal(page.called("createProgram").length, 1);
});

test("a new box is fitted just before the next frame, never between frames", () => {
  const page = running();
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
  assert.deepEqual(page.told("u_size"), [800, 500]);
  // A new box is no new scene: the graphics card is not asked again.
  assert.equal(page.called("getError").length, 1);
});

test("a paused scene follows its box, and asks for no frame once it fits", () => {
  const page = running();
  page.pause.emit("click");
  const stopped = page.told("u_time");
  const slid = page.told("u_drift");
  page.win.emit("pointermove", { pointerType: "mouse", clientX: 1000, clientY: 0 });
  page.canvas.clientWidth = 500;
  page.canvas.clientHeight = 300;
  page.observers.resized();
  // As when it runs, it is fitted by the frame that paints it.
  assert.equal(page.canvas.width, 1000);
  assert.equal(page.waiting, 1);
  page.tick();
  assert.deepEqual([page.canvas.width, page.canvas.height], [500, 300]);
  assert.equal(page.called("drawArrays").length, 2);
  assert.equal(page.waiting, 0);
  // Nothing in the picture has moved: not the sky, and not after the mouse.
  assert.deepEqual(page.told("u_time"), stopped);
  assert.deepEqual(page.told("u_drift"), slid);
  // A box that is gone is left alone.
  page.canvas.clientWidth = 0;
  page.observers.resized();
  page.tick();
  assert.equal(page.canvas.width, 500);
  assert.equal(page.waiting, 0);
});

test("a paused scene that had to leave the page comes back paused, with its button", () => {
  // The graphics card went away and came back.
  const lost = running();
  lost.pause.emit("click");
  const stopped = lost.told("u_time");
  lost.gl.lost = true;
  lost.canvas.emit("webglcontextlost", { preventDefault() {} });
  assert.equal(lost.pause.hidden, true);
  lost.gl.lost = false;
  lost.canvas.emit("webglcontextrestored");
  lost.tick();
  assert.equal(lost.live, true);
  assert.equal(lost.pause.hidden, false);
  assert.equal(lost.pause.pressed, "true");
  assert.deepEqual(lost.told("u_time"), stopped);
  assert.equal(lost.waiting, 0);
  // The visitor asked for less motion, and then for it back.
  const calmed = running();
  calmed.pause.emit("click");
  calmed.media.matches = true;
  calmed.media.emit("change");
  assert.equal(calmed.live, false);
  calmed.media.matches = false;
  calmed.media.emit("change");
  calmed.tick();
  assert.equal(calmed.live, true);
  assert.equal(calmed.pause.hidden, false);
  assert.equal(calmed.pause.pressed, "true");
  assert.equal(calmed.waiting, 0);
  // Its button still starts it again.
  calmed.pause.emit("click");
  calmed.tick();
  calmed.tick();
  assert.equal(calmed.pause.pressed, "false");
  assert.equal(calmed.waiting, 1);
});

test("a canvas with no box yet is painted only once it has one", () => {
  const page = stand({ width: 0 });
  enliven(page.win);
  page.tick();
  page.tick();
  assert.equal(page.called("drawArrays").length, 0);
  assert.equal(page.called("viewport").length, 0);
  assert.equal(page.live, false);
  page.canvas.clientWidth = 1000;
  page.observers.resized();
  page.tick();
  // Fitted and painted in the one frame.
  assert.equal(page.called("viewport").length, 1);
  assert.equal(page.called("drawArrays").length, 1);
  assert.equal(page.live, true);
  // A box with no height is no box either.
  const flat = stand({ height: 0 });
  enliven(flat.win);
  flat.tick();
  assert.equal(flat.called("viewport").length, 0);
  assert.equal(flat.live, false);
  flat.canvas.clientHeight = 600;
  flat.observers.resized();
  flat.tick();
  assert.equal(flat.live, true);
});

test("a denser screen is followed", () => {
  const page = running();
  page.win.devicePixelRatio = 2;
  page.tick();
  assert.deepEqual([page.canvas.width, page.canvas.height], [2000, 1200]);
  assert.deepEqual(page.told("u_unit"), [0.8]);
});

test("the mouse slides the sky, a finger does not", () => {
  const page = running();
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
