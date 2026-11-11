// When the home page's live scene runs, and what the page shows when it
// does not. The page works without it: the drawn image is already there,
// and the scene is painted over it only where the browser has WebGL 2 and
// the visitor has not asked for less motion. Where the scene cannot be
// made, or runs too slowly, the drawn image stays. A fault in the page or
// in this code is thrown, not hidden. Why the scene is not showing is left
// on the hero as `data-scene`, for whoever wonders: one of the `WHY` below.
//
// The scene is worked out by the graphics card alone (`scene.mjs`), so
// there is nothing to load and nothing to wait for: it is made at once.
//
// Everything that reaches the canvas goes through `frame`: it alone fits
// the buffer and paints. A paused scene is the same scene with time
// standing still: it is given a frame whenever the one it shows is missing
// or no longer fits, and no others.
//
// Everything is reached through the window handed to `enliven`, so the
// whole of it can be run against a stand-in (`test/stand.mjs`).

import { anchor, buffer, cover, numbers, watch, watching } from "./measure.mjs";
import { createScene } from "./scene.mjs";

/** Below this many milliseconds since the last frame, a frame is skipped. */
const SOONEST = 11;
/** How far the pointer slides the sky, in picture units each way. */
const REACH = { x: 8, y: 3 };
/** What the hero says of its scene: that it shows, or why it does not. */
const WHY = Object.freeze({
  live: "live",
  calm: "reduced-motion",
  unable: "no-webgl2",
  slow: "too-slow",
  refused: "refused",
  lost: "context-lost",
});

function need(root, selector) {
  const found = root.querySelector(selector);
  if (!found) {
    throw new Error(`the home page has no ${selector}`);
  }
  return found;
}

/** The picture's size, which the page wrote on the scene's canvas. */
function brief(canvas) {
  const view = canvas.dataset.view;
  if (view === undefined) {
    throw new Error("the scene's canvas has no data-view");
  }
  const [width, height] = numbers(view, 2);
  return { width, height };
}

/** Brings the hero of the page in `win` to life, where it can be. */
export function enliven(win) {
  const hero = need(win.document, ".hero");
  const art = need(hero, ".hero-art");
  const canvas = need(hero, "canvas.hero-scene");
  const pause = need(hero, ".hero-pause");
  const picture = brief(canvas);
  const still = win.matchMedia("(prefers-reduced-motion: reduce)");
  const state = {
    gl: null,
    scene: null,
    /** The scene has been asked for; it is asked for once. */
    begun: false,
    /** The scene was made and given up; it is not made again. */
    over: false,
    /** What the hero was last told of the scene: one of `WHY`, or nothing. */
    told: "",
    seen: true,
    paused: false,
    request: 0,
    last: 0,
    seconds: 0,
    watchdog: watching(),
    ratio: 0,
    density: 0,
    /** The canvas's box has changed since the buffer was fitted to it. */
    unfit: false,
    /**
     * The scene has been made, or made again, since the graphics card was
     * last asked what it refused.
     */
    unproven: false,
    aim: { x: 0, y: 0 },
    slid: { x: 0, y: 0 },
  };

  /**
   * Shows the scene over the drawn image when `why` is that it is live,
   * and the drawn image alone for any other.
   */
  function show(why) {
    if (state.told !== why) {
      state.told = why;
      const live = why === WHY.live;
      hero.classList.toggle("is-live", live);
      pause.hidden = !live;
      hero.dataset.scene = why;
    }
  }

  /** Sizes the drawing buffer to the canvas and tells the scene its view. */
  function fit() {
    state.unfit = false;
    const box = { width: canvas.clientWidth, height: canvas.clientHeight };
    if (box.width === 0 || box.height === 0) {
      return;
    }
    state.ratio = win.devicePixelRatio;
    const size = buffer(box, state.ratio, state.watchdog.quality);
    if (canvas.width !== size.width || canvas.height !== size.height) {
      canvas.width = size.width;
      canvas.height = size.height;
    }
    const view = cover(box, picture, anchor(win.getComputedStyle(art).objectPosition));
    state.density = view.scale * size.scale;
    state.scene.setView(size.width, size.height, { x: view.left, y: view.top }, 1 / state.density);
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

  /** The scene has proved too slow, or cannot be made here: `why`. */
  function giveUp(why) {
    state.over = true;
    halt();
    show(why);
    state.scene.dispose();
    state.scene = null;
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
    let lowered = false;
    if (!state.paused) {
      // Only a scene that moves is judged for its speed.
      const { state: watchdog, verdict } = watch(state.watchdog, gap);
      if (verdict === "stop") {
        giveUp(WHY.slow);
        return;
      }
      lowered = verdict === "lower";
      state.watchdog = watchdog;
    }
    // The buffer is sized here, just before it is painted, and never
    // between frames: sizing it clears it.
    if (state.unfit || lowered || state.ratio !== win.devicePixelRatio) {
      fit();
    }
    if (state.density === 0) {
      // The canvas has no box yet, so there is nothing to paint on.
      return;
    }
    if (!state.paused) {
      // Time stands still while the page is away, so the scene does not
      // jump.
      const step = Math.min(gap, 100);
      state.seconds += step / 1000;
      const ease = 1 - Math.exp(-step / 600);
      state.slid.x += (state.aim.x - state.slid.x) * ease;
      state.slid.y += (state.aim.y - state.slid.y) * ease;
    }
    paint();
    if (state.unproven) {
      // Asked once for each time the scene is made, not once a frame: the
      // answer has to be waited for.
      state.unproven = false;
      const fault = state.scene.fault();
      if (fault !== null) {
        giveUp(WHY.refused);
        throw new Error(`the graphics card refused the scene: WebGL error ${fault}`);
      }
    }
    if (state.gl.isContextLost()) {
      // Nothing was painted. The event that says so is on its way, and
      // stops the frames.
      return;
    }
    show(WHY.live);
    // A paused scene asks for no more once the frame it shows is right.
    run();
  }

  /** Starts or stops the frames to match what the page and visitor want. */
  function run() {
    const inView = state.scene !== null && !still.matches && state.seen && !win.document.hidden;
    // A paused scene is still owed a frame when it shows none, or when its
    // box has changed under the one it shows.
    const owed = state.told !== WHY.live || state.unfit;
    const wanted = inView && (!state.paused || owed);
    if (wanted && state.request === 0) {
      state.last = 0;
      state.request = win.requestAnimationFrame(frame);
    } else if (!wanted && state.request !== 0) {
      halt();
    }
  }

  /** Makes the scene on the context, which may be new or just restored. */
  function build() {
    let made = null;
    try {
      made = createScene(state.gl);
      state.scene = made;
      state.unproven = true;
      fit();
    } catch (error) {
      state.scene = null;
      if (state.gl.isContextLost()) {
        // The graphics card went away half-way, and took what was made
        // with it; its return builds again.
        return;
      }
      made?.dispose();
      throw error;
    }
    run();
  }

  function begin() {
    if (state.begun) {
      return;
    }
    if (still.matches) {
      show(WHY.calm);
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
      show(WHY.unable);
      return;
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
    show(WHY.lost);
  });
  canvas.addEventListener("webglcontextrestored", () => {
    if (!state.over && state.gl) {
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
      show(WHY.calm);
    } else {
      run();
      begin();
    }
  });

  win.document.addEventListener("visibilitychange", run);

  new win.IntersectionObserver((entries) => {
    state.seen = entries[entries.length - 1].isIntersecting;
    run();
  }).observe(hero);

  new win.ResizeObserver(() => {
    // The next frame fits it; a paused scene is owed that frame.
    state.unfit = true;
    run();
  }).observe(canvas);

  win.addEventListener("pointermove", (event) => {
    if (event.pointerType === "mouse") {
      state.aim.x = (event.clientX / win.innerWidth - 0.5) * 2 * REACH.x;
      state.aim.y = (event.clientY / win.innerHeight - 0.5) * 2 * REACH.y;
    }
  }, { passive: true });

  begin();
}
