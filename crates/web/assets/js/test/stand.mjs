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
 * misspelt call fails here and not only in a browser, and so does a call
 * the scene has no business making: it holds no texture and no buffer.
 */
const METHODS = new Set([
  "attachShader", "bindVertexArray", "compileShader", "createProgram", "createShader",
  "createVertexArray", "deleteProgram", "deleteShader", "deleteVertexArray", "detachShader",
  "disable", "drawArrays", "linkProgram", "shaderSource", "uniform1f", "uniform2f", "useProgram",
  "viewport",
]);
const CONSTANTS = new Set([
  "BLEND", "COMPILE_STATUS", "CONTEXT_LOST_WEBGL", "DEPTH_TEST", "FRAGMENT_SHADER", "LINK_STATUS",
  "NO_ERROR", "TRIANGLES", "VERTEX_SHADER",
]);

/** The error WebGL keeps for a call that its state does not allow. */
const INVALID_OPERATION = 0x502;

function needsProgram(own) {
  if (own.program === null) {
    own.refuse();
  }
}

/**
 * The state that decides whether a call is allowed, kept as WebGL keeps
 * it: the program in use. A call made out of order is refused the way a
 * graphics card refuses it: nothing happens, and an error waits to be
 * asked for.
 */
const STATE = {
  useProgram(own, program) {
    own.program = program;
  },
  uniform1f: needsProgram,
  uniform2f: needsProgram,
  drawArrays: needsProgram,
};

/** A WebGL context that records what it is asked and refuses on request. */
function graphics(settings) {
  const own = {
    calls: [],
    lost: false,
    error: 0,
    program: null,
    refuse() {
      own.error ||= INVALID_OPERATION;
    },
    isContextLost: () => own.lost,
    // As WebGL answers: the error kept, which is then forgotten.
    getError() {
      own.calls.push(["getError"]);
      const kept = own.error;
      own.error = 0;
      return kept;
    },
    // A lost context answers nothing, whatever it was asked.
    getShaderParameter: () => settings.compiles && !own.lost,
    getProgramParameter: () => settings.links && !own.lost,
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
        STATE[name]?.(target, ...given);
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
    position: "62% 100%",
    width: 1000,
    height: 600,
    ...options,
  };
  const pending = new Map();
  const classes = new Set();
  const observers = {};
  let ids = 0;
  let now = 1000;
  const gl = graphics(settings);
  const media = Object.assign(new Target(), { matches: settings.reduced });
  const canvas = Object.assign(new Target(), {
    dataset: { view: "1600 1000", ...settings.dataset },
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
  const art = {};
  const parts = { ".hero-art": art, "canvas.hero-scene": canvas };
  const hero = {
    dataset: {},
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
  // The scene makes nothing on the page and loads nothing: the document
  // answers only what it is asked to find.
  const document = Object.assign(new Target(), {
    hidden: false,
    querySelector: (selector) => (selector === ".hero" && settings.missing !== selector ? hero : null),
  });
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
    IntersectionObserver,
    ResizeObserver,
    devicePixelRatio: 1,
    innerWidth: 1000,
    innerHeight: 600,
    matchMedia(query) {
      assert.equal(query, "(prefers-reduced-motion: reduce)");
      return media;
    },
    getComputedStyle(element) {
      assert.equal(element, art);
      return { objectPosition: settings.position };
    },
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
    win, gl, canvas, media, document, observers,
    get live() {
      return classes.has("is-live");
    },
    /** What the hero says of its scene. */
    get why() {
      return hero.dataset.scene;
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
    /** Every uniform the scene has been told, by name. */
    uniforms() {
      const names = gl.calls.filter(([call]) => call.startsWith("uniform")).map(([, name]) => name);
      return [...new Set(names)].toSorted();
    },
  };
}

/** A page whose scene has painted its first frame. */
export function running(options) {
  const page = stand(options);
  enliven(page.win);
  page.tick();
  assert.equal(page.live, true);
  return page;
}
