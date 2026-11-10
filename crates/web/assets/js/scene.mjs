// The live scene's drawing: one program, one triangle, two layer textures.
// It knows WebGL and nothing of the page; `stage.mjs` tells it where and
// when to draw.

import { FRAGMENT, VERTEX } from "./shader.mjs";

const UNIFORMS = [
  "u_size", "u_origin", "u_unit", "u_time", "u_drift", "u_picture", "u_pole", "u_span",
  "u_shore", "u_lantern", "u_land", "u_ring",
];

/** A compiled shader. A shader that does not compile is a fault in this code. */
function shader(gl, type, source) {
  const made = gl.createShader(type);
  gl.shaderSource(made, source);
  gl.compileShader(made);
  if (!gl.getShaderParameter(made, gl.COMPILE_STATUS) && !gl.isContextLost()) {
    const log = gl.getShaderInfoLog(made);
    gl.deleteShader(made);
    throw new Error(`the scene's shader did not compile: ${log}`);
  }
  return made;
}

function program(gl) {
  const made = gl.createProgram();
  const parts = [shader(gl, gl.VERTEX_SHADER, VERTEX), shader(gl, gl.FRAGMENT_SHADER, FRAGMENT)];
  for (const part of parts) {
    gl.attachShader(made, part);
  }
  gl.linkProgram(made);
  for (const part of parts) {
    gl.detachShader(made, part);
    gl.deleteShader(part);
  }
  if (!gl.getProgramParameter(made, gl.LINK_STATUS) && !gl.isContextLost()) {
    const log = gl.getProgramInfoLog(made);
    gl.deleteProgram(made);
    throw new Error(`the scene's program did not link: ${log}`);
  }
  return made;
}

/**
 * An empty texture on `unit`, not repeated, and smoothed between its
 * smaller copies: the water mirrors the layers out of those.
 */
function texture(gl, unit) {
  const made = gl.createTexture();
  gl.activeTexture(gl.TEXTURE0 + unit);
  gl.bindTexture(gl.TEXTURE_2D, made);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return made;
}

/**
 * The scene on `gl`. `shape` is the picture's geometry in its own units:
 * `picture` (width and height), `pole`, `span` (the side of the ring
 * layer's square), `shore` and `lantern`.
 */
export function createScene(gl, shape) {
  const made = program(gl);
  const at = Object.fromEntries(UNIFORMS.map((name) => [name, gl.getUniformLocation(made, name)]));
  const triangle = gl.createVertexArray();
  const layers = { land: texture(gl, 0), ring: texture(gl, 1) };
  gl.useProgram(made);
  gl.bindVertexArray(triangle);
  gl.disable(gl.DEPTH_TEST);
  gl.disable(gl.BLEND);
  // The layers are drawn through a 2D canvas, which holds colour already
  // multiplied by its opacity; keep it so, and blend that way.
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
  gl.uniform1i(at.u_land, 0);
  gl.uniform1i(at.u_ring, 1);
  gl.uniform2f(at.u_picture, shape.picture.width, shape.picture.height);
  gl.uniform2f(at.u_pole, shape.pole.x, shape.pole.y);
  gl.uniform1f(at.u_span, shape.span);
  gl.uniform1f(at.u_shore, shape.shore);
  gl.uniform2f(at.u_lantern, shape.lantern.x, shape.lantern.y);

  return {
    /** The widest layer this graphics card will hold, in pixels. */
    largest: gl.getParameter(gl.MAX_TEXTURE_SIZE),

    /**
     * Replaces the layer `name` ("land" or "ring") with `pixels`, a canvas.
     * Throws a `SecurityError` where the browser will not let a page read
     * back a drawn SVG.
     */
    setLayer(name, pixels) {
      gl.activeTexture(name === "land" ? gl.TEXTURE0 : gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, layers[name]);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      gl.generateMipmap(gl.TEXTURE_2D);
    },

    /**
     * The buffer is `width` by `height` pixels; its top-left pixel is at
     * `origin` in the picture, and each pixel spans `unit` picture units.
     */
    setView(width, height, origin, unit) {
      gl.viewport(0, 0, width, height);
      gl.uniform2f(at.u_size, width, height);
      gl.uniform2f(at.u_origin, origin.x, origin.y);
      gl.uniform1f(at.u_unit, unit);
    },

    /** Draws the scene `seconds` into its run, the sky slid by `drift`. */
    draw(seconds, drift) {
      gl.uniform1f(at.u_time, seconds);
      gl.uniform2f(at.u_drift, drift.x, drift.y);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    },

    /**
     * What the graphics card has refused since it was last asked, as a
     * WebGL error code, or `null` if nothing. A lost context is not a
     * refusal: the page is told of that by an event.
     */
    fault() {
      const code = gl.getError();
      return code === gl.NO_ERROR || code === gl.CONTEXT_LOST_WEBGL
        ? null
        : `0x${code.toString(16)}`;
    },

    /** Gives back what the scene holds on the graphics card. */
    dispose() {
      gl.deleteTexture(layers.land);
      gl.deleteTexture(layers.ring);
      gl.deleteVertexArray(triangle);
      gl.deleteProgram(made);
    },
  };
}
