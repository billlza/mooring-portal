// Stand-ins for what the translator uses on the host: fastText's command
// line, the engine's script and its WebAssembly, the models, and the
// connection systemd hands over.

import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after } from "node:test";

/** A directory of its own for one test, removed after the tests. */
export function place() {
  const directory = mkdtempSync(join(tmpdir(), "translator-"));
  after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

/**
 * A stand-in for fastText's command line in `directory`: it writes down
 * its arguments and what it read, prints `answer` and ends with `status`.
 * With `reads` false it reads nothing and ends at once.
 */
export function fasttext(directory, answer, { status = 0, reads = true } = {}) {
  const program = join(directory, "fasttext");
  writeFileSync(join(directory, "answer"), answer);
  const read = reads ? 'cat > "$here/input"' : "";
  writeFileSync(program, `#!/bin/sh\nhere=$(dirname "$0")\nprintf '%s\\n' "$@" > "$here/args"\n${read}\ncat "$here/answer"\nexit ${status}\n`);
  chmodSync(program, 0o755);
  return program;
}

/**
 * The engine's script as a stand-in: the classes the translator uses, as
 * the engine names them, writing down what they are given in
 * `globalThis.translatorStand` and translating as it says.
 */
const ENGINE = `
function loadBergamot(Module) {
  // A translator started as a process has no test to set it up.
  const stand = globalThis.translatorStand ??
    (globalThis.translatorStand = { abort: null, translate: (texts) => texts.map((text) => "[" + text + "]"),
      started: [], models: [], services: [], asked: [] });
  stand.started.push({ memory: Module.INITIAL_MEMORY, wasm: Module.wasmBinary.length });
  Module.print("words of the text");
  Module.printErr("words of the text");
  class AlignedMemory {
    constructor(size, alignment) { this.bytes = new Uint8Array(size); this.alignment = alignment; }
    getByteArrayView() { return this.bytes; }
  }
  class List {
    constructor() { this.items = []; }
    push_back(item) { this.items.push(item); }
    size() { return this.items.length; }
  }
  class TranslationModel {
    constructor(from, into, settings, model, lex, vocabularies, quality) {
      stand.models.push({ from, into, settings, model, lex, vocabularies: vocabularies.items, quality });
    }
  }
  class BlockingService {
    constructor(config) { stand.services.push(config); }
    translate(model, texts, options) {
      stand.asked.push({ texts: texts.items, options: options.items });
      const said = stand.translate(texts.items);
      return { size: () => said.length, get: (at) => ({ getTranslatedText: () => said[at] }) };
    }
  }
  Object.assign(Module, { AlignedMemory, AlignedMemoryList: List, TranslationModel, BlockingService,
    VectorString: List, VectorResponseOptions: List });
  setTimeout(() => (stand.abort === null ? Module.onRuntimeInitialized() : Module.onAbort(stand.abort)), 0);
  return Module;
}
`;

/** A stand-in engine in `directory`, and what it is given. */
export function engine(directory, { abort = null, translate = (texts) => texts.map((text) => `[${text}]`) } = {}) {
  const engineDirectory = join(directory, "engine");
  mkdirSync(engineDirectory);
  writeFileSync(join(engineDirectory, "bergamot-translator.js"), ENGINE);
  writeFileSync(join(engineDirectory, "bergamot-translator.wasm"), Buffer.from("\0asm\x01\0\0\0"));
  globalThis.translatorStand = { abort, translate, started: [], models: [], services: [], asked: [] };
  return { directory: engineDirectory, stand: globalThis.translatorStand };
}

/** A model in `models` for `from`-`into`, with the files `names`, each holding its own name. */
export function model(models, from, into, names) {
  const directory = join(models, `${from}-${into}`);
  mkdirSync(directory, { recursive: true });
  for (const name of names) {
    writeFileSync(join(directory, name), `the bytes of ${name}`);
  }
  return directory;
}

/** Firefox's Chinese-English model's files, and its English-Chinese one's. */
export const ONE_VOCABULARY = ["model.zhen.intgemm.alphas.bin", "lex.50.50.zhen.s2t.bin", "vocab.zhen.spm"];
export const TWO_VOCABULARIES = [
  "model.enzh.intgemm.alphas.bin",
  "lex.50.50.enzh.s2t.bin",
  "srcvocab.enzh.spm",
  "trgvocab.enzh.spm",
];

/**
 * A connection like the one systemd hands over, and the portal's end of
 * it: `answer` is all the portal reads until the translator ends it.
 */
export async function connection(directory) {
  const path = join(directory, "socket");
  const server = net.createServer();
  await new Promise((resolve) => server.listen(path, resolve));
  const handed = new Promise((resolve) => server.once("connection", resolve));
  const portal = net.connect(path);
  const chunks = [];
  portal.on("data", (chunk) => chunks.push(chunk));
  const answer = new Promise((resolve) => portal.on("close", () => resolve(Buffer.concat(chunks))));
  const translator = await handed;
  server.close();
  return { translator, portal, answer };
}
