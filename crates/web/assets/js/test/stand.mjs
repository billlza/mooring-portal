// A stand-in for the home page, for the tests of the live scene: the
// hero's elements, a window around them, a clock the test turns, and a
// graphics card that records what it is asked. No browser is needed.

import assert from "node:assert/strict";

import { enliven } from "../stage.mjs";

class Target {
  #handlers = new Map();

  addEventListener(type, handler) {
    this.#handlers.set(type, [...(this.#handlers.get(type) ?? []), handler]);
  }

  emit(type, event = {}) {
    for (const handler of this.#handlers.get(type) ?? []) {
      handler(event);
    }
    return event;
  }
}

/**
 * The WebGL 2 methods and constants the scene may use, by their names in
 * the specification. The stand-in below answers to no other name, so a
 * misspelt call fails here and not only in a browser.
 */
const METHODS = new Set([
  "activeTexture", "attachShader", "bindTexture", "bindVertexArray", "compileShader",
  "createProgram", "createShader", "createTexture", "createVertexArray", "deleteProgram",
  "deleteShader", "deleteTexture", "deleteVertexArray", "detachShader", "disable", "drawArrays",
  "generateMipmap", "linkProgram", "pixelStorei", "shaderSource", "texImage2D", "texParameteri",
  "uniform1f", "uniform1i", "uniform2f", "useProgram", "viewport",
]);
const CONSTANTS = new Set([
  "BLEND", "CLAMP_TO_EDGE", "COMPILE_STATUS", "CONTEXT_LOST_WEBGL", "DEPTH_TEST",
  "FRAGMENT_SHADER", "LINEAR", "LINEAR_MIPMAP_LINEAR", "LINK_STATUS", "MAX_TEXTURE_SIZE",
  "NO_ERROR", "RGBA", "TEXTURE0", "TEXTURE1", "TEXTURE_2D", "TEXTURE_MAG_FILTER",
  "TEXTURE_MIN_FILTER", "TEXTURE_WRAP_S", "TEXTURE_WRAP_T", "TRIANGLES",
  "UNPACK_PREMULTIPLY_ALPHA_WEBGL", "UNSIGNED_BYTE", "VERTEX_SHADER",
]);

/** A WebGL context that records what it is asked and refuses on request. */
function graphics(settings) {
  const own = {
    calls: [],
    lost: false,
    error: 0,
    isContextLost: () => own.lost,
    getError: () => own.error,
    getParameter: (name) => (name === "MAX_TEXTURE_SIZE" ? settings.largest : null),
    getShaderParameter: () => settings.compiles,
    getProgramParameter: () => settings.links,
    getShaderInfoLog: () => "a shader fault",
    getProgramInfoLog: () => "a linking fault",
    getUniformLocation: (_program, name) => name,
  };
  return new Proxy(own, {
    get(target, name) {
      if (name in target) {
        return target[name];
      }
      if (CONSTANTS.has(name)) {
        return name === "NO_ERROR" ? 0 : name;
      }
      if (!METHODS.has(name)) {
        throw new TypeError(`WebGL 2 has no ${String(name)}`);
      }
      return (...given) => {
        target.calls.push([name, ...given]);
        settings.during?.[name]?.(target);
        return {};
      };
    },
  });
}

/** A home page: the hero's elements, a window around them, and a clock. */
export function stand(options = {}) {
  const settings = {
    reduced: false,
    webgl: true,
    compiles: true,
    links: true,
    largest: 4096,
    arrive: () => Promise.resolve(),
    pen: true,
    width: 1000,
    height: 600,
    ...options,
  };
  const pending = new Map();
  const classes = new Set();
  const loaded = [];
  const sheets = [];
  const scratch = [];
  const observers = {};
  let ids = 0;
  let now = 1000;
  const gl = graphics(settings);
  const media = Object.assign(new Target(), { matches: settings.reduced });
  const canvas = Object.assign(new Target(), {
    dataset: {
      view: "1600 1000",
      pole: "1100 500",
      span: "760",
      shore: "800",
      lantern: "1215 768",
      land: "/assets/land.svg",
      ring: "/assets/ring.svg",
      ...settings.dataset,
    },
    clientWidth: settings.width,
    clientHeight: settings.height,
    width: 300,
    height: 150,
    asked: [],
    getContext(kind, wanted) {
      this.asked.push([kind, wanted]);
      return settings.webgl ? gl : null;
    },
  });
  const pause = Object.assign(new Target(), {
    hidden: true,
    pressed: "false",
    setAttribute(name, value) {
      assert.equal(name, "aria-pressed");
      this.pressed = value;
    },
  });
  const art = {};
  const parts = { ".hero-art": art, "canvas.hero-scene": canvas, ".hero-pause": pause };
  const hero = {
    classList: {
      toggle(name, on) {
        if (on) {
          classes.add(name);
        } else {
          classes.delete(name);
        }
      },
    },
    querySelector: (selector) => (settings.missing === selector ? null : parts[selector]),
  };
  const document = Object.assign(new Target(), {
    hidden: false,
    querySelector: (selector) => (selector === ".hero" && settings.missing !== selector ? hero : null),
    createElement(tag) {
      assert.equal(tag, "canvas");
      const pen = {
        drawImage(image, x, y, width, height) {
          sheets.push({ image: image.src, at: [x, y], width, height });
        },
      };
      const sheet = {
        width: 0,
        height: 0,
        getContext: (kind) => (kind === "2d" && settings.pen ? pen : null),
      };
      scratch.push(sheet);
      return sheet;
    },
  });
  class Image {
    decode() {
      loaded.push(this.src);
      return settings.arrive();
    }
  }
  class IntersectionObserver {
    constructor(callback) {
      observers.seen = callback;
    }

    observe(target) {
      assert.equal(target, hero);
    }
  }
  class ResizeObserver {
    constructor(callback) {
      observers.resized = callback;
    }

    observe(target) {
      assert.equal(target, canvas);
    }
  }
  const win = Object.assign(new Target(), {
    document,
    Image,
    IntersectionObserver,
    ResizeObserver,
    DOMException,
    devicePixelRatio: 1,
    innerWidth: 1000,
    innerHeight: 600,
    matchMedia(query) {
      assert.equal(query, "(prefers-reduced-motion: reduce)");
      return media;
    },
    getComputedStyle(element) {
      assert.equal(element, art);
      return { objectPosition: "62% 100%" };
    },
    performance: { now: () => now },
    requestAnimationFrame(callback) {
      ids += 1;
      pending.set(ids, callback);
      return ids;
    },
    cancelAnimationFrame(id) {
      pending.delete(id);
    },
  });
  return {
    win, gl, canvas, pause, media, document, observers, loaded, sheets, scratch,
    get live() {
      return classes.has("is-live");
    },
    /** How many frames the page is waiting for. */
    get waiting() {
      return pending.size;
    },
    /** Runs the frames asked for, `ms` after the last. */
    tick(ms = 16.7) {
      now += ms;
      const due = [...pending.values()];
      pending.clear();
      for (const callback of due) {
        callback(now);
      }
    },
    /** The calls made to the graphics card named `name`. */
    called(name) {
      return gl.calls.filter(([call]) => call === name);
    },
    /** What the scene was last told for the uniform `name`. */
    told(name) {
      return gl.calls.findLast(([, uniform]) => uniform === name).slice(2);
    },
    /** The texture unit each layer was sent to, in the order sent. */
    uploads() {
      let unit = "";
      const units = [];
      for (const [name, value] of gl.calls) {
        if (name === "activeTexture") {
          unit = value;
        } else if (name === "texImage2D") {
          units.push(unit);
        }
      }
      return units;
    },
  };
}

/** A page whose scene has painted its first frame. */
export async function running(options) {
  const page = stand(options);
  await enliven(page.win);
  page.tick();
  assert.equal(page.live, true);
  return page;
}

export const settle = () => new Promise((resolve) => setImmediate(resolve));
