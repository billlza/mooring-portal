// What the translator asks of fastText and of the engine, against
// stand-ins for both (`stand.mjs`): how a text goes to fastText and what
// is made of its answer, which files make a model, how a model goes into
// the engine, and what is kept of what the engine prints.
// Run with `node --test "crates/portal/translator/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import {
  Failure,
  MIN_CONFIDENCE,
  identifier,
  modelFiles,
  settingsFor,
  startEngine,
  translateWith,
  translator,
} from "../translator.mjs";
import { ONE_VOCABULARY, TWO_VOCABULARIES, engine, fasttext, model, place } from "./stand.mjs";

/** The failure `promise` rejects with, by its words. */
async function refusal(promise) {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof Failure, String(error));
    return error.message;
  }
  assert.fail("no failure");
}

test("fastText is given the text as one line, and its language is said when it is sure enough of it", async () => {
  const directory = place();
  const program = fasttext(directory, "__label__ja 0.98\n");
  const identify = identifier(program, "/lid.176.ftz");
  assert.equal(await identify("こんにちは\n  二行目\t\r\nです "), "ja");
  assert.deepEqual(readFileSync(join(directory, "args"), "utf8").split("\n"), ["predict-prob", "/lid.176.ftz", "-", "1", ""]);
  assert.equal(readFileSync(join(directory, "input"), "utf8"), "こんにちは 二行目 です\n");
  // Not sure enough, sure enough, and sure beyond one as it may print.
  for (const [answer, said] of [
    [`__label__en ${MIN_CONFIDENCE - 0.01}\n`, null],
    [`__label__en ${MIN_CONFIDENCE}\n`, "en"],
    ["__label__yue 1.00004\n", "yue"],
    ["__label__en 1e-05\n", null],
  ]) {
    assert.equal(await identifier(fasttext(place(), answer), "/model")("text"), said, answer);
  }
});

test("a text of white space alone is in no language, and fastText is not asked", async () => {
  const directory = place();
  const identify = identifier(fasttext(directory, "__label__en 0.9\n"), "/model");
  assert.equal(await identify(" \n\t　 "), null);
  assert.throws(() => readFileSync(join(directory, "args")), { code: "ENOENT" });
});

test("fastText that answers something else, or fails, or cannot be run, leaves the question unanswered", async () => {
  for (const answer of ["", "__label__en\n", "__label__en 0.9", "__label__en 0.9\n__label__fr 0.1\n",
    "__label__EN 0.9\n", "__label__zh-Hans 0.9\n", "__label__en 0,9\n", "en 0.9\n"]) {
    const reason = await refusal(identifier(fasttext(place(), answer), "/model")("text"));
    assert.equal(reason, "fastText did not name one language and how sure it is", answer);
  }
  const failing = fasttext(place(), "", { status: 3 });
  assert.equal(await refusal(identifier(failing, "/model")("text")), `${failing} ended with 3`);
  const missing = join(place(), "no-fasttext");
  assert.equal(await refusal(identifier(missing, "/model")("text")), `${missing} could not be run: ENOENT`);
  const talkative = fasttext(place(), `${"__label__en 0.9\n".repeat(20)}`);
  assert.equal(await refusal(identifier(talkative, "/model")("text")), `${talkative} wrote more than an answer`);
  // One that answers without reading what it was given.
  const deaf = fasttext(place(), "__label__en 0.9\n", { reads: false });
  assert.match(await refusal(identifier(deaf, "/model")("x".repeat(1 << 20))), /^.* did not read the text: EPIPE$/);
});

test("a model is its file, its shortlist if it has one, and one vocabulary or one for each side", () => {
  const models = place();
  assert.equal(modelFiles(models, "ko", "en"), null);
  const zhen = model(models, "zh", "en", ONE_VOCABULARY);
  assert.deepEqual(modelFiles(models, "zh", "en"), {
    lex: join(zhen, "lex.50.50.zhen.s2t.bin"),
    model: join(zhen, "model.zhen.intgemm.alphas.bin"),
    vocab: join(zhen, "vocab.zhen.spm"),
  });
  const enzh = model(models, "en", "zh", TWO_VOCABULARIES);
  assert.deepEqual(Object.keys(modelFiles(models, "en", "zh")).sort(), ["lex", "model", "srcvocab", "trgvocab"]);
  assert.equal(modelFiles(models, "en", "zh").srcvocab, join(enzh, "srcvocab.enzh.spm"));
  model(models, "ru", "en", ["model.ruen.intgemm.alphas.bin", "vocab.ruen.spm"]);
  assert.deepEqual(Object.keys(modelFiles(models, "ru", "en")).sort(), ["model", "vocab"]);
  for (const [names, reason] of [
    [["vocab.x.spm"], "lacks a file"],
    [["model.x.bin"], "lacks a file"],
    [["model.x.bin", "srcvocab.x.spm"], "lacks a file"],
    [["model.x.bin", "vocab.x.spm", "srcvocab.x.spm", "trgvocab.x.spm"], "lacks a file"],
    [["model.x.bin", "vocab.x.spm", "README"], "holds a file it should not"],
    [["model.x.bin", "model.y.bin", "vocab.x.spm"], "holds a file it should not"],
    [["model.x.bin", "vocab.x.spm", ".DS_Store"], "holds a file it should not"],
  ]) {
    const each = place();
    model(each, "es", "en", names);
    assert.throws(() => modelFiles(each, "es", "en"), (error) => error instanceof Failure && error.message === `the model es-en ${reason}`, names.join(" "));
  }
  // A file where the model's directory should be.
  writeFileSync(join(models, "ja-en"), "");
  assert.throws(() => modelFiles(models, "ja", "en"), (error) => error instanceof Failure && error.message === "the model ja-en cannot be read: ENOTDIR");
});

test("the engine is given Firefox's settings, and a model of 8-bit weights alone is computed as Firefox does", () => {
  const settings = settingsFor("/models/zh-en/model.zhen.intgemm.alphas.bin");
  const indent = "            ";
  assert.ok(settings.startsWith(`\n${indent}beam-size: 1\n`));
  assert.ok(settings.endsWith(`\n${indent}alignment: soft\n${indent}`));
  assert.match(settings, /\n {12}gemm-precision: int8shiftAlphaAll\n/);
  assert.match(settings, /\n {12}skip-cost: true\n/);
  assert.equal(settings.split("\n").length, 15);
  assert.match(settingsFor("/models/x/model.xen.intgemm8.bin"), /\n {12}gemm-precision: int8shiftAll\n/);
});

test("a model's files go into the engine's memory as they are, aligned as Firefox aligns them, and the text is translated with it", async () => {
  const directory = place();
  const { directory: engineDirectory, stand } = engine(directory);
  const models = join(directory, "models");
  model(models, "en", "zh", TWO_VOCABULARIES);
  const logged = [];
  const kept = { log: console.log, error: console.error };
  console.log = console.error = (...words) => logged.push(words.join(" "));
  let bergamot;
  try {
    bergamot = await startEngine(engineDirectory);
  } finally {
    Object.assign(console, kept);
  }
  // What the engine printed went nowhere.
  assert.deepEqual(logged, []);
  assert.deepEqual(stand.started, [{ memory: 41_943_040, wasm: 8 }]);
  const files = modelFiles(models, "en", "zh");
  assert.equal(translateWith(bergamot, files, "en", "zh", "Hello"), "[Hello]");
  const [made] = stand.models;
  assert.equal(made.from, "en");
  assert.equal(made.into, "zh");
  assert.equal(made.settings, settingsFor(files.model));
  assert.equal(made.quality, null);
  const memory = (kind) => ({ alignment: kind.alignment, bytes: Buffer.from(kind.bytes).toString() });
  // Firefox aligns a model to 256 bytes, and the rest to 64.
  assert.deepEqual(memory(made.model), { alignment: 256, bytes: "the bytes of model.enzh.intgemm.alphas.bin" });
  assert.deepEqual(memory(made.lex), { alignment: 64, bytes: "the bytes of lex.50.50.enzh.s2t.bin" });
  // The source's vocabulary first.
  assert.deepEqual(made.vocabularies.map(memory), [
    { alignment: 64, bytes: "the bytes of srcvocab.enzh.spm" },
    { alignment: 64, bytes: "the bytes of trgvocab.enzh.spm" },
  ]);
  assert.deepEqual(stand.services, [{ cacheSize: 0 }]);
  assert.deepEqual(stand.asked, [{ texts: ["Hello"], options: [{ qualityScores: false, alignment: false, html: false }] }]);
});

test("a model without a shortlist goes in without one, and an engine that answers for no text or for two fails", async () => {
  const directory = place();
  const { directory: engineDirectory, stand } = engine(directory, { translate: () => [] });
  const models = join(directory, "models");
  model(models, "ru", "en", ["model.ruen.intgemm.alphas.bin", "vocab.ruen.spm"]);
  const bergamot = await startEngine(engineDirectory);
  const files = modelFiles(models, "ru", "en");
  assert.throws(() => translateWith(bergamot, files, "ru", "en", "Привет"), (error) => error.message === "the engine did not answer for the one text it was given");
  assert.equal(stand.models[0].lex, null);
  stand.translate = () => ["one", "two"];
  assert.throws(() => translateWith(bergamot, files, "ru", "en", "Привет"), Failure);
});

test("the translator says nothing of a direction it has no model for, and starts the engine only for one it has", async () => {
  const directory = place();
  const { directory: engineDirectory, stand } = engine(directory);
  const models = join(directory, "models");
  model(models, "ja", "en", ["model.jaen.intgemm.alphas.bin", "lex.50.50.jaen.s2t.bin", "vocab.jaen.spm"]);
  const translate = translator(engineDirectory, models);
  assert.equal(await translate("ko", "en", "안녕하세요"), null);
  assert.deepEqual(stand.started, []);
  assert.equal(await translate("ja", "en", "こんにちは"), "[こんにちは]");
  assert.equal(stand.started.length, 1);
});

test("an engine that stops, or cannot be read, leaves the question unanswered, and says why in printable words", async () => {
  const directory = place();
  const { directory: engineDirectory } = engine(directory, { abort: "OOM\nwith 文字" });
  assert.equal(await refusal(startEngine(engineDirectory)), "the engine stopped: OOM?with ??");
  const empty = join(place(), "engine");
  mkdirSync(empty);
  assert.throws(() => startEngine(empty), (error) => error instanceof Failure && error.message === "the engine cannot be read: ENOENT");
});
