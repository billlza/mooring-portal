// The host's translator, for the portal's `translate` command
// (docs/translation.md). systemd starts it for each connection to the
// translator's socket and hands it the connection as descriptor 3
// (`Accept=yes`). It reads one question, answers it and ends. It is handed
// a text and nothing of whose it is, and it reaches neither the network
// nor the portal's files: its unit forbids both.
//
// A question is a line that says what is asked and how many bytes of text
// follow, then the text, in UTF-8:
//
//     detect <bytes>\n<text>
//     translate <from> <into> <bytes>\n<text>
//
// The answer is `ok <bytes>\n` and what was asked for in so many bytes, or
// `none\n` when there is nothing to say: the language cannot be told, or
// no model goes that way. A question that cannot be answered for any other
// reason gets no answer: the connection ends, the portal counts a failure
// and asks again on its next run, and the reason goes to the journal.
// Nothing of the text ever does.
//
// fastText's command line tells the language, with fastText's model for
// it. Mozilla's engine for Firefox's translations (bergamot-translator,
// built to WebAssembly) translates, with Firefox's models: each goes into
// English or out of it, and the portal asks for one direction at a time.
// Chinese in traditional characters has a model of its own, and fastText
// calls it Chinese all the same, so its characters tell it apart.
//
// The host runs Debian's Node 20, so nothing newer is used.

import { spawn } from "node:child_process";
import { closeSync, fstatSync, openSync, readFileSync, readSync, readdirSync, realpathSync } from "node:fs";
import net from "node:net";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import vm from "node:vm";

/** The longest first line of a question: `translate`, two languages and a length. */
export const MAX_LINE_BYTES = 64;

/**
 * The longest text of a question. The portal asks about messages of 1000
 * characters at most, and has translations of 6000 at most put from
 * English into another language; no character takes more than 4 bytes.
 */
export const MAX_TEXT_BYTES = 24_000;

/** How sure fastText must be of a language for it to be said. */
export const MIN_CONFIDENCE = 0.5;

/** The most fastText writes about one line: a label and a number. */
const MAX_IDENTIFICATION_BYTES = 256;

/** A language as the portal and fastText name it. */
const LANGUAGE = /^[a-z]{2,3}$/;

/** The files of a model by kind, with the alignment Firefox gives each in the engine's memory. */
export const ALIGNMENT = { model: 256, lex: 64, vocab: 64, srcvocab: 64, trgvocab: 64 };

/** The engine's memory to begin with, as Firefox has it: it grows as a model needs. */
const INITIAL_MEMORY = 41_943_040;

/** What went wrong, in words that hold nothing of the text. */
export class Failure extends Error {}

/** Nothing to say. */
export const NOTHING = Buffer.from("none\n");

/** The answer that says `content`. */
export function said(content) {
  const bytes = Buffer.from(content, "utf8");
  return Buffer.concat([Buffer.from(`ok ${bytes.length}\n`), bytes]);
}

/** What the first line `line` of a question asks, and how many bytes of text it says follow. */
export function parseLine(line) {
  const words = line.split(" ");
  const length = words.pop();
  let asked;
  if (words.length === 1 && words[0] === "detect") {
    asked = { ask: "detect" };
  } else if (words.length === 3 && words[0] === "translate" && LANGUAGE.test(words[1]) && LANGUAGE.test(words[2])) {
    asked = { ask: "translate", from: words[1], into: words[2] };
  } else {
    throw new Failure("the question asks neither detect nor translate");
  }
  if (!/^[1-9][0-9]*$/.test(length)) {
    throw new Failure("the question does not say in digits how many bytes of text follow");
  }
  const bytes = Number(length);
  if (bytes > MAX_TEXT_BYTES) {
    throw new Failure("the question's text is longer than any the portal asks about");
  }
  return { ...asked, length: bytes };
}

/** `bytes` as text, if they are UTF-8. */
function utf8(bytes) {
  try {
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    throw new Failure("the question's text is not UTF-8");
  }
}

/**
 * Reads a question as its bytes come: `push` gives it once all of it has
 * come, and throws as soon as the bytes cannot be one.
 */
export class QuestionReader {
  #bytes = Buffer.alloc(0);
  #asked = null;

  push(chunk) {
    this.#bytes = Buffer.concat([this.#bytes, chunk]);
    if (this.#asked === null) {
      const end = this.#bytes.indexOf(0x0a);
      if (end > MAX_LINE_BYTES || (end === -1 && this.#bytes.length > MAX_LINE_BYTES)) {
        throw new Failure("the question's first line is too long");
      }
      if (end === -1) {
        return null;
      }
      this.#asked = parseLine(this.#bytes.subarray(0, end).toString("latin1"));
      this.#bytes = this.#bytes.subarray(end + 1);
    }
    if (this.#bytes.length > this.#asked.length) {
      throw new Failure("more text came than the question says");
    }
    if (this.#bytes.length < this.#asked.length) {
      return null;
    }
    return { ...this.#asked, text: utf8(this.#bytes) };
  }
}

/** The question that comes on `connection`. */
export function readQuestion(connection) {
  return new Promise((resolve, reject) => {
    const reader = new QuestionReader();
    const stop = (settle, value) => {
      connection.off("data", onData);
      connection.off("end", onEnd);
      connection.off("error", onError);
      connection.pause();
      settle(value);
    };
    const onData = (chunk) => {
      let question;
      try {
        question = reader.push(chunk);
      } catch (error) {
        stop(reject, error);
        return;
      }
      if (question !== null) {
        stop(resolve, question);
      }
    };
    const onEnd = () => stop(reject, new Failure("the connection ended before the question did"));
    const onError = (error) => stop(reject, new Failure(`the connection failed: ${error.code ?? error.message}`));
    connection.on("data", onData);
    connection.on("end", onEnd);
    connection.on("error", onError);
  });
}

/** The answer to `question`, from `tools.identify` and `tools.translate`. */
export async function answerTo(question, tools) {
  const answer =
    question.ask === "detect"
      ? await tools.identify(question.text)
      : await tools.translate(question.from, question.into, question.text);
  return answer === null ? NOTHING : said(answer);
}

/**
 * What `command` writes on its standard output when it is given `input`,
 * at most `limit` bytes of it. What it writes on its standard error goes
 * to the journal with this program's.
 */
function output(command, args, input, limit) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ["pipe", "pipe", "inherit"] });
    let written = Buffer.alloc(0);
    let unread = null;
    child.stdout.on("data", (chunk) => {
      written = Buffer.concat([written, chunk]);
      if (written.length > limit) {
        child.kill();
      }
    });
    child.stdin.on("error", (error) => {
      unread = error;
    });
    child.on("error", (error) => reject(new Failure(`${command} could not be run: ${error.code ?? error.message}`)));
    child.on("close", (status, signal) => {
      if (written.length > limit) {
        reject(new Failure(`${command} wrote more than an answer`));
      } else if (status !== 0) {
        reject(new Failure(`${command} ended with ${signal ?? status}`));
      } else if (unread !== null) {
        reject(new Failure(`${command} did not read the text: ${unread.code ?? unread.message}`));
      } else {
        resolve(written.toString("utf8"));
      }
    });
    child.stdin.end(input);
  });
}

/**
 * Tells the language of a text with fastText's command line `fasttext`
 * and its model `model`: the language it finds most likely, if it is sure
 * enough of it. fastText reads a line at a time, so the text goes as one.
 */
export function identifier(fasttext, model) {
  return async (text) => {
    const line = text.replace(/\s+/gu, " ").trim();
    if (line === "") {
      return null;
    }
    const answer = await output(fasttext, ["predict-prob", model, "-", "1"], `${line}\n`, MAX_IDENTIFICATION_BYTES);
    const found = /^__label__([a-z]{2,3}) ([0-9]+(?:\.[0-9]+)?(?:e[-+]?[0-9]+)?)\n$/.exec(answer);
    if (found === null) {
      throw new Failure("fastText did not name one language and how sure it is");
    }
    return Number(found[2]) >= MIN_CONFIDENCE ? found[1] : null;
  };
}

/** A Chinese character, as the decoders give one. */
const HAN = /^\p{Script=Han}$/u;

/** The numbers of the bytes in the ranges `ranges`, each of a first and a last. */
function* bytesIn(ranges) {
  for (const [first, last] of ranges) {
    for (let byte = first; byte <= last; byte += 1) {
      yield byte;
    }
  }
}

/**
 * The Chinese characters of a two-byte character set: what Node's decoder
 * for `label` reads in each pair of a byte of `leads` and one of `trails`.
 */
function hanzi(label, leads, trails) {
  const decoder = new TextDecoder(label);
  const characters = new Set();
  for (const lead of bytesIn(leads)) {
    for (const trail of bytesIn(trails)) {
      const character = decoder.decode(Uint8Array.of(lead, trail));
      if (HAN.test(character)) {
        characters.add(character);
      }
    }
  }
  return characters;
}

/**
 * Whether Chinese `text` is written in traditional characters (`Hant`) or
 * in simplified ones (`Hans`). fastText calls both `zh`, and Firefox has a
 * model for each. A character of GB 2312, the mainland's set, that Big5,
 * Taiwan's, lacks is simplified; one of Big5 that GB 2312 lacks is
 * traditional; one both have says nothing. The text is in the script more
 * of its characters are in, and in simplified ones when neither has more:
 * the characters of such a text are the simplified model's as well.
 */
export function chineseScript(text) {
  const gb2312 = hanzi("gbk", [[0xb0, 0xf7]], [[0xa1, 0xfe]]);
  const big5 = hanzi("big5", [[0xa4, 0xc6], [0xc9, 0xf9]], [[0x40, 0x7e], [0xa1, 0xfe]]);
  let balance = 0;
  for (const character of text) {
    if (big5.has(character) !== gb2312.has(character)) {
      balance += big5.has(character) ? 1 : -1;
    }
  }
  return balance > 0 ? "Hant" : "Hans";
}

/**
 * The files, by kind, of the model in `models` that goes from `from` into
 * `into`, or null when none is installed. A model has one file of each
 * kind it uses: the model itself, a shortlist if it has one, and one
 * vocabulary or one for each side.
 */
export function modelFiles(models, from, into) {
  const directory = join(models, `${from}-${into}`);
  let names;
  try {
    names = readdirSync(directory);
  } catch (error) {
    if (error.code === "ENOENT") {
      return null;
    }
    throw new Failure(`the model ${from}-${into} cannot be read: ${error.code ?? error.message}`);
  }
  const files = {};
  for (const name of names.sort()) {
    const kind = Object.keys(ALIGNMENT).find((each) => name.startsWith(`${each}.`));
    if (kind === undefined || kind in files) {
      throw new Failure(`the model ${from}-${into} holds a file it should not`);
    }
    files[kind] = join(directory, name);
  }
  const vocabularies = ["vocab", "srcvocab", "trgvocab"].filter((kind) => kind in files).join(" ");
  if (!("model" in files) || (vocabularies !== "vocab" && vocabularies !== "srcvocab trgvocab")) {
    throw new Failure(`the model ${from}-${into} lacks a file`);
  }
  return files;
}

/**
 * Firefox's settings for its models, in the form the engine reads
 * (`translations-engine.worker.js`). A model whose weights are 8-bit
 * integers without their scales is named so, and computed differently.
 */
export function settingsFor(model) {
  const settings = {
    "beam-size": "1",
    normalize: "1.0",
    "word-penalty": "0",
    "max-length-break": "128",
    "mini-batch-words": "1024",
    workspace: "128",
    "max-length-factor": "2.0",
    "skip-cost": "true",
    "cpu-threads": "0",
    quiet: "true",
    "quiet-translation": "true",
    "gemm-precision": model.endsWith("intgemm8.bin") ? "int8shiftAll" : "int8shiftAlphaAll",
    alignment: "soft",
  };
  const indent = "            ";
  return `\n${Object.entries(settings).map(([key, value]) => `${indent}${key}: ${value}\n`).join("")}${indent}`;
}

/** Printable ASCII of `what`, briefly: it is written to the journal. */
function plain(what) {
  return String(what).replace(/[^\x20-\x7e]/g, "?").slice(0, 200);
}

/**
 * Starts the engine whose script and WebAssembly are in `engine`. What the
 * engine itself prints is not kept: it could hold words of the text.
 */
export function startEngine(engine) {
  const script = join(engine, "bergamot-translator.js");
  let source;
  let wasmBinary;
  try {
    source = readFileSync(script, "utf8");
    wasmBinary = readFileSync(join(engine, "bergamot-translator.wasm"));
  } catch (error) {
    throw new Failure(`the engine cannot be read: ${error.code ?? error.message}`);
  }
  const loadBergamot = vm.runInThisContext(`${source}\n;loadBergamot`, { filename: script });
  return new Promise((resolve, reject) => {
    const bergamot = loadBergamot({
      INITIAL_MEMORY,
      print: () => {},
      printErr: () => {},
      onAbort: (what) => reject(new Failure(`the engine stopped: ${plain(what)}`)),
      onRuntimeInitialized: () => {
        // As Firefox does: `bergamot` is assigned once the call returns.
        Promise.resolve().then(() => resolve(bergamot));
      },
      wasmBinary,
    });
  });
}

/** Copies the file `path` into the engine's memory, aligned so. */
function aligned(bergamot, path, alignment) {
  const file = openSync(path, "r");
  try {
    const size = fstatSync(file).size;
    const memory = new bergamot.AlignedMemory(size, alignment);
    const view = memory.getByteArrayView();
    for (let done = 0; done < size; ) {
      const read = readSync(file, view, done, size - done, done);
      if (read === 0) {
        throw new Failure(`${path} ended before its size`);
      }
      done += read;
    }
    return memory;
  } finally {
    closeSync(file);
  }
}

/** `text`, written in `from`, in `into`, by the model of the files `files` on the started engine `bergamot`. */
export function translateWith(bergamot, files, from, into, text) {
  const memory = {};
  for (const [kind, path] of Object.entries(files)) {
    memory[kind] = aligned(bergamot, path, ALIGNMENT[kind]);
  }
  const vocabularies = new bergamot.AlignedMemoryList();
  for (const kind of ["vocab", "srcvocab", "trgvocab"]) {
    if (kind in memory) {
      vocabularies.push_back(memory[kind]);
    }
  }
  const model = new bergamot.TranslationModel(
    from,
    into,
    settingsFor(files.model),
    memory.model,
    memory.lex ?? null,
    vocabularies,
    null,
  );
  const service = new bergamot.BlockingService({ cacheSize: 0 });
  const texts = new bergamot.VectorString();
  texts.push_back(text);
  const options = new bergamot.VectorResponseOptions();
  options.push_back({ qualityScores: false, alignment: false, html: false });
  const responses = service.translate(model, texts, options);
  if (responses.size() !== 1) {
    throw new Failure("the engine did not answer for the one text it was given");
  }
  return responses.get(0).getTranslatedText();
}

/**
 * Translates with the engine in `engine` and the models in `models`.
 * Chinese in traditional characters goes to Firefox's model for them,
 * `zh-Hant-en`, and to none when it is not installed.
 */
export function translator(engine, models) {
  return async (from, into, text) => {
    const source = from === "zh" && chineseScript(text) === "Hant" ? "zh-Hant" : from;
    const files = modelFiles(models, source, into);
    if (files === null) {
      return null;
    }
    return translateWith(await startEngine(engine), files, source, into, text);
  };
}

/** Where the engine, the models and fastText are, from the command line `argv`. */
export function optionsFrom(argv) {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        engine: { type: "string" },
        models: { type: "string" },
        fasttext: { type: "string" },
        identifier: { type: "string" },
      },
      strict: true,
      allowPositionals: false,
    }));
  } catch (error) {
    throw new Failure(`the command line is wrong: ${error.message}`);
  }
  for (const name of ["engine", "models", "fasttext", "identifier"]) {
    if (!values[name]) {
      throw new Failure(`the command line lacks --${name}`);
    }
  }
  return values;
}

/** The connection systemd hands this process, the process `pid`, in the environment `env`. */
export function handedConnection(env, pid) {
  if (env.LISTEN_FDS !== "1" || env.LISTEN_PID !== String(pid)) {
    throw new Failure("systemd did not hand over one connection (LISTEN_FDS and LISTEN_PID)");
  }
  return new net.Socket({ fd: 3, readable: true, writable: true });
}

/**
 * Reads the question on `connection`, answers it there with `tools`, and
 * lets go of the connection.
 *
 * The answer does not end by closing the writing half. systemd keeps the
 * connection it hands over until this process has ended, and counts it
 * against `MaxConnections=1` until then. An answer ended by a half-close
 * reached the portal while this process was still ending, and the portal's
 * next question found the connection still counted and was dropped
 * (2026-11-23, on the host). Without one, the portal reads the end of the
 * answer only when systemd lets go of the connection too, and by then the
 * next question is let in.
 */
export async function answerOne(connection, tools) {
  try {
    const question = await readQuestion(connection);
    const answer = await answerTo(question, tools);
    await new Promise((resolve, reject) => {
      const failed = (error) => reject(new Failure(`the answer could not be written: ${error.code ?? error.message}`));
      connection.once("error", failed);
      connection.write(answer, (error) => (error ? failed(error) : resolve()));
    });
  } finally {
    connection.destroy();
  }
}

/** Answers the question on the connection systemd hands over; the status to end with. */
export async function run(argv, env, pid, log) {
  try {
    const options = optionsFrom(argv);
    await answerOne(handedConnection(env, pid), {
      identify: identifier(options.fasttext, options.identifier),
      translate: translator(options.engine, options.models),
    });
    return 0;
  } catch (error) {
    const reason = error instanceof Failure ? error.message : `${error.name}: ${plain(error.message)}`;
    log(`mooring-portal-translator: ${reason}`);
    return 1;
  }
}

if (process.argv[1] !== undefined && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) {
  process.exitCode = await run(process.argv.slice(2), process.env, process.pid, (line) => console.error(line));
}
