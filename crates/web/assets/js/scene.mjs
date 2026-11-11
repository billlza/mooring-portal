// The live scene's drawing: one program and one triangle that covers the
// view. It knows WebGL and nothing of the page; `stage.mjs` tells it where
// and when to draw. The whole picture is worked out by the shader, pixel by
// pixel: nothing is loaded, and there is no texture to wait for.

import { FRAGMENT, VERTEX } from "./shader.mjs";

const UNIFORMS = ["u_size", "u_origin", "u_unit", "u_time", "u_drift"];

/**
 * A compiled shader. A shader that does not compile is a fault in this
 * code, unless the context has been lost, which answers nothing either:
 * whoever asked for the scene tells the two apart.
 */
function shader(gl, type, source) {
  const made = gl.createShader(type);
  gl.shaderSource(made, source);
  gl.compileShader(made);
  if (!gl.getShaderParameter(made, gl.COMPILE_STATUS)) {
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
  if (!gl.getProgramParameter(made, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(made);
    gl.deleteProgram(made);
    throw new Error(`the scene's program did not link: ${log}`);
  }
  return made;
}

/** The scene on `gl`. */
export function createScene(gl) {
  const made = program(gl);
  const at = Object.fromEntries(UNIFORMS.map((name) => [name, gl.getUniformLocation(made, name)]));
  const triangle = gl.createVertexArray();
  gl.useProgram(made);
  gl.bindVertexArray(triangle);
  gl.disable(gl.DEPTH_TEST);
  gl.disable(gl.BLEND);

  return {
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
      gl.deleteVertexArray(triangle);
      gl.deleteProgram(made);
    },
  };
}
