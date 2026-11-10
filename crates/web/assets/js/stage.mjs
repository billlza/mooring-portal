// When the home page's live scene runs, and what the page shows when it
// does not. The page works without it: the drawn image is already there,
// and the scene is painted over it only where the browser has WebGL 2 and
// the visitor has not asked for less motion. Where the scene cannot be
// made, or runs too slowly, the drawn image stays. A fault in the page or
// in this code is thrown, not hidden.
//
// Everything is reached through the window handed to `enliven`, so the
// whole of it can be run against a stand-in (`test/stand.mjs`).

import { anchor, buffer, cover, layer, numbers, watch, watching } from "./measure.mjs";
import { createScene } from "./scene.mjs";

/** Below this many milliseconds since the last frame, a frame is skipped. */
const SOONEST = 11;
/** How long the view must hold still before the layers are drawn afresh. */
const SETTLE_MS = 300;
/** How far the pointer slides the sky, in picture units each way. */
const REACH = { x: 8, y: 3 };
/** The sizes the layers may be drawn at, in pixels across. */
const LAND = { least: 640, most: 2400 };
const RING = { least: 256, most: 1536 };

function need(root, selector) {
  const found = root.querySelector(selector);
  if (!found) {
    throw new Error(`the home page has no ${selector}`);
  }
  return found;
}

/** What the page wrote on the scene's canvas: the layers and the geometry. */
function brief(canvas) {
  const told = (name) => {
    const value = canvas.dataset[name];
    if (value === undefined) {
      throw new Error(`the scene's canvas has no data-${name}`);
    }
    return value;
  };
  const [width, height] = numbers(told("view"), 2);
  const [poleX, poleY] = numbers(told("pole"), 2);
  const [lanternX, lanternY] = numbers(told("lantern"), 2);
  return {
    shape: {
      picture: { width, height },
      pole: { x: poleX, y: poleY },
      span: numbers(told("span"), 1)[0],
      shore: numbers(told("shore"), 1)[0],
      lantern: { x: lanternX, y: lanternY },
    },
    sources: { land: told("land"), ring: told("ring") },
  };
}

/**
 * Brings the hero of the page in `win` to life. The promise settles once
 * the scene has been made, or it is known that it will not be.
 */
export function enliven(win) {
  const hero = need(win.document, ".hero");
  const art = need(hero, ".hero-art");
  const canvas = need(hero, "canvas.hero-scene");
  const pause = need(hero, ".hero-pause");
  const { shape, sources } = brief(canvas);
  const still = win.matchMedia("(prefers-reduced-motion: reduce)");
  const state = {
    gl: null,
    scene: null,
    images: null,
    /** The scene has been asked for; it is asked for once. */
    begun: false,
    /** The scene was made and given up; it is not made again. */
    over: false,
    live: false,
    seen: true,
    paused: false,
    request: 0,
    last: 0,
    seconds: 0,
    watchdog: watching(),
    ratio: 0,
    density: 0,
    sizes: { land: "", ring: "" },
    /** The canvas's box has changed since the buffer was fitted to it. */
    unfit: false,
    fittedAt: 0,
    stale: false,
    /** The first frame since the scene was made has yet to be checked. */
    unproven: true,
    aim: { x: 0, y: 0 },
    slid: { x: 0, y: 0 },
  };

  function picture(url) {
    const image = new win.Image();
    image.decoding = "async";
    image.src = url;
    return image.decode().then(() => image);
  }

  /** Shows the scene over the drawn image, or the drawn image alone. */
  function show(live) {
    if (state.live !== live) {
      state.live = live;
      hero.classList.toggle("is-live", live);
      pause.hidden = !live;
    }
  }

  /** Sizes the drawing buffer to the canvas and tells the scene its view. */
  function fit() {
    state.unfit = false;
    const box = { width: canvas.clientWidth, height: canvas.clientHeight };
    if (box.width === 0 || box.height === 0) {
      return false;
    }
    state.ratio = win.devicePixelRatio;
    const size = buffer(box, state.ratio, state.watchdog.quality);
    if (canvas.width !== size.width || canvas.height !== size.height) {
      canvas.width = size.width;
      canvas.height = size.height;
    }
    const view = cover(box, shape.picture, anchor(win.getComputedStyle(art).objectPosition));
    state.density = view.scale * size.scale;
    state.scene.setView(size.width, size.height, { x: view.left, y: view.top }, 1 / state.density);
    state.fittedAt = win.performance.now();
    state.stale = true;
    return true;
  }

  /** Draws the layers at the size the view now needs, if that has changed. */
  function dress() {
    state.stale = false;
    const within = (bounds) => ({ ...bounds, most: Math.min(bounds.most, state.scene.largest) });
    const wanted = {
      land: layer(shape.picture, state.density, within(LAND)),
      ring: layer({ width: shape.span, height: shape.span }, state.density, within(RING)),
    };
    for (const name of ["land", "ring"]) {
      const key = `${wanted[name].width}x${wanted[name].height}`;
      if (state.sizes[name] !== key) {
        const sheet = win.document.createElement("canvas");
        sheet.width = wanted[name].width;
        sheet.height = wanted[name].height;
        const pen = sheet.getContext("2d");
        if (!pen) {
          throw new Error("no 2D canvas to draw the scene's layers with");
        }
        pen.drawImage(state.images[name], 0, 0, sheet.width, sheet.height);
        state.scene.setLayer(name, sheet);
        // Its pixels are on the graphics card now; let go of this copy.
        sheet.width = 0;
        sheet.height = 0;
        state.sizes[name] = key;
      }
    }
  }

  function paint() {
    const sway = {
      x: 5 * Math.sin(state.seconds * 0.07),
      y: 2 * Math.sin(state.seconds * 0.05 + 1),
    };
    state.scene.draw(state.seconds, { x: state.slid.x + sway.x, y: state.slid.y + sway.y });
  }

  function halt() {
    win.cancelAnimationFrame(state.request);
    state.request = 0;
  }

  /** The scene has proved too slow, or cannot be made here. */
  function giveUp() {
    state.over = true;
    halt();
    show(false);
    if (state.scene) {
      state.scene.dispose();
      state.scene = null;
    }
  }

  function frame(now) {
    state.request = win.requestAnimationFrame(frame);
    if (state.last === 0) {
      state.last = now;
    }
    const gap = now - state.last;
    if (gap !== 0 && gap < SOONEST) {
      return;
    }
    state.last = now;
    const { state: watchdog, verdict } = watch(state.watchdog, gap);
    if (verdict === "stop") {
      giveUp();
      return;
    }
    const lowered = verdict === "lower";
    state.watchdog = watchdog;
    // The buffer is sized here, just before it is painted, and never
    // between frames: sizing it clears it.
    if (state.unfit || lowered || state.ratio !== win.devicePixelRatio) {
      fit();
    }
    if (state.density === 0) {
      // The canvas has no box yet, so there is nothing to paint on.
      return;
    }
    const undressed = state.sizes.land === "";
    if (state.stale && (undressed || now - state.fittedAt > SETTLE_MS)) {
      dress();
    }
    // Time stands still while the page is away, so the sky does not jump.
    const step = Math.min(gap, 100);
    state.seconds += step / 1000;
    const ease = 1 - Math.exp(-step / 600);
    state.slid.x += (state.aim.x - state.slid.x) * ease;
    state.slid.y += (state.aim.y - state.slid.y) * ease;
    paint();
    if (state.unproven) {
      state.unproven = false;
      const fault = state.scene.fault();
      if (fault !== null) {
        giveUp();
        throw new Error(`the scene's first frame failed: WebGL error ${fault}`);
      }
    }
    show(true);
  }

  /** Starts or stops the frames to match what the page and visitor want. */
  function run() {
    const wanted = state.scene !== null && !still.matches && !state.paused && state.seen
      && !win.document.hidden;
    if (wanted && state.request === 0) {
      state.last = 0;
      state.request = win.requestAnimationFrame(frame);
    } else if (!wanted && state.request !== 0) {
      halt();
    }
  }

  /** Makes the scene on the context, which may be new or just restored. */
  function build() {
    try {
      state.scene = createScene(state.gl, shape);
      state.sizes = { land: "", ring: "" };
      state.unproven = true;
      if (fit()) {
        dress();
      }
    } catch (error) {
      const made = state.scene;
      state.scene = null;
      if (state.gl.isContextLost()) {
        // The graphics card went away half-way, and took what was made
        // with it; its return builds again.
        return;
      }
      made?.dispose();
      if (error instanceof win.DOMException && error.name === "SecurityError") {
        // This browser treats a drawn SVG as another site's picture.
        giveUp();
        return;
      }
      throw error;
    }
    run();
  }

  async function begin() {
    if (state.begun || still.matches) {
      return;
    }
    state.begun = true;
    // No software rendering: a scene drawn by the processor would cost
    // more than it is worth.
    state.gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      failIfMajorPerformanceCaveat: true,
    });
    if (!state.gl) {
      return;
    }
    try {
      const [land, ring] = await Promise.all([picture(sources.land), picture(sources.ring)]);
      state.images = { land, ring };
    } catch (error) {
      if (error instanceof win.DOMException && error.name === "EncodingError") {
        // A layer did not arrive, or could not be read.
        return;
      }
      throw error;
    }
    if (!state.gl.isContextLost()) {
      build();
    }
  }

  canvas.addEventListener("webglcontextlost", (event) => {
    // Asking for the context back; until then the drawn image shows.
    event.preventDefault();
    halt();
    state.scene = null;
    show(false);
  });
  canvas.addEventListener("webglcontextrestored", () => {
    if (!state.over && state.images) {
      build();
    }
  });

  pause.addEventListener("click", () => {
    state.paused = !state.paused;
    pause.setAttribute("aria-pressed", String(state.paused));
    run();
  });

  still.addEventListener("change", () => {
    if (still.matches) {
      halt();
      show(false);
    } else {
      run();
      void begin();
    }
  });

  win.document.addEventListener("visibilitychange", run);

  new win.IntersectionObserver((entries) => {
    state.seen = entries[entries.length - 1].isIntersecting;
    run();
  }).observe(hero);

  new win.ResizeObserver(() => {
    state.unfit = true;
    if (state.scene && state.request === 0 && !still.matches && fit()) {
      // Paused or out of sight: keep the still frame fitting its box.
      dress();
      paint();
    }
  }).observe(canvas);

  win.addEventListener("pointermove", (event) => {
    if (event.pointerType === "mouse") {
      state.aim.x = (event.clientX / win.innerWidth - 0.5) * 2 * REACH.x;
      state.aim.y = (event.clientY / win.innerHeight - 0.5) * 2 * REACH.y;
    }
  }, { passive: true });

  return begin();
}
