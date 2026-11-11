// Sizes, positions and pacing for the home page's live scene: arithmetic
// only, with no page and no drawing, so it can be tested on its own
// (`test/measure.test.mjs`).

/** What the scene may cost. */
export const LIMIT = Object.freeze({
  /** Screen densities above this are not followed. */
  ratio: 2,
  /** The most pixels drawn in a frame. */
  pixels: 3_200_000,
  /**
   * A scene that averages more than this between frames is too slow: under
   * 24 frames a second. A browser saving power draws 30, which is enough.
   */
  frameMs: 42,
  /** A longer gap is the page having been away, not a slow frame. */
  gapMs: 250,
});

/** Frames that are not judged after a start or a change of quality. */
export const WARM_UP = 30;
/** Frames averaged for a verdict. */
export const WINDOW = 90;
/** The lowest quality; a scene too slow at it is stopped. */
export const FLOOR = 0.5;

const NUMBER = /^-?\d+(\.\d+)?$/;
const ANCHOR = /^(-?\d+(?:\.\d+)?)% (-?\d+(?:\.\d+)?)%$/;

/**
 * The `count` numbers in `text`, which the page wrote separated by spaces.
 * Anything else is a fault in the page, not something to draw around.
 */
export function numbers(text, count) {
  const parts = text.trim().split(/\s+/);
  if (parts.length !== count || !parts.every((part) => NUMBER.test(part))) {
    throw new Error(`expected ${count} numbers, found ${JSON.stringify(text)}`);
  }
  return parts.map(Number);
}

/**
 * A computed `object-position` as fractions of the spare room: "62% 100%"
 * is `[0.62, 1]`. The stylesheet positions the picture in percentages;
 * lengths and keywords are not read.
 */
export function anchor(text) {
  const match = ANCHOR.exec(text.trim().replace(/\s+/g, " "));
  if (!match) {
    throw new Error(`object-position is not two percentages: ${JSON.stringify(text)}`);
  }
  return [Number(match[1]) / 100, Number(match[2]) / 100];
}

/**
 * How `picture` fills `box` under `object-fit: cover`, anchored at `at`:
 * the box's pixels per picture unit, and where in the picture the box's
 * top-left corner falls. The box must have an area.
 */
export function cover(box, picture, at) {
  const scale = Math.max(box.width / picture.width, box.height / picture.height);
  return {
    scale,
    left: ((picture.width * scale - box.width) * at[0]) / scale,
    top: ((picture.height * scale - box.height) * at[1]) / scale,
  };
}

/**
 * The drawing buffer for a `box` of CSS pixels on a screen of density
 * `ratio`, at the watchdog's `quality`: device pixels, within
 * [`LIMIT`]. `scale` is buffer pixels per CSS pixel.
 */
export function buffer(box, ratio, quality) {
  let scale = Math.min(ratio, LIMIT.ratio) * quality;
  const pixels = box.width * box.height * scale * scale;
  if (pixels > LIMIT.pixels) {
    scale *= Math.sqrt(LIMIT.pixels / pixels);
  }
  // Rounded down, so the budget holds; the nudge keeps 1499.9999… at 1500.
  const width = Math.max(1, Math.floor(box.width * scale + 1e-6));
  const height = Math.max(1, Math.floor(box.height * scale + 1e-6));
  return { width, height, scale: box.width > 0 ? width / box.width : scale };
}

/** A watchdog that has judged nothing yet, for a scene at `quality`. */
export function watching(quality = 1) {
  return { quality, warm: WARM_UP, frames: 0, total: 0 };
}

/**
 * The watchdog after a frame that came `ms` after the one before, and its
 * verdict: "keep", "lower" (the state carries the new, lower quality) or
 * "stop" (too slow even at the lowest: show the drawn image instead).
 */
export function watch(state, ms) {
  if (ms > LIMIT.gapMs) {
    return { state, verdict: "keep" };
  }
  if (state.warm > 0) {
    return { state: { ...state, warm: state.warm - 1 }, verdict: "keep" };
  }
  const frames = state.frames + 1;
  const total = state.total + ms;
  if (frames < WINDOW) {
    return { state: { ...state, frames, total }, verdict: "keep" };
  }
  if (total / frames <= LIMIT.frameMs) {
    return { state: { ...state, frames: 0, total: 0 }, verdict: "keep" };
  }
  if (state.quality <= FLOOR) {
    return { state, verdict: "stop" };
  }
  return { state: watching(Math.max(FLOOR, state.quality - 0.25)), verdict: "lower" };
}
