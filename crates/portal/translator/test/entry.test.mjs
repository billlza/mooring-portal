// The translator as systemd runs it (docs/translation.md, section 5): a
// process handed a connection as descriptor 3, which answers and ends, or
// says why it cannot; its command line in the notes; and the files of
// `files.txt`, where it looks for them.
// Run with `node --test "crates/portal/translator/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFileSync, symlinkSync } from "node:fs";
import net from "node:net";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { modelFiles, optionsFrom, run } from "../translator.mjs";
import { engine, fasttext, model, place } from "./stand.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(HERE, "..", "translator.mjs");
const NOTES = readFileSync(join(HERE, "../../../../docs/translation.md"), "utf8");
const FILES = readFileSync(join(HERE, "..", "files.txt"), "utf8");

/**
 * The translator `script` started as systemd starts it: its own process,
 * told so in its environment, with the connection as descriptor 3.
 */
function started(script, directory) {
  const { directory: engineDirectory } = engine(directory);
  const models = join(directory, "models");
  model(models, "ja", "en", ["model.jaen.intgemm.alphas.bin", "vocab.jaen.spm"]);
  const child = spawn("sh", ["-c", 'LISTEN_PID=$$ LISTEN_FDS=1 exec "$0" "$@"', process.execPath, script,
    "--engine", engineDirectory, "--models", models,
    "--fasttext", fasttext(directory, "__label__ja 0.97\n"), "--identifier", "/lid.176.ftz"],
  { stdio: ["ignore", "ignore", "pipe", "pipe"] });
  const said = [];
  const errors = [];
  child.stdio[3].on("data", (chunk) => said.push(chunk));
  child.stderr.on("data", (chunk) => errors.push(chunk));
  const ended = new Promise((resolve) => child.on("close", (status) => resolve({
    status,
    answer: Buffer.concat(said).toString("utf8"),
    journal: Buffer.concat(errors).toString("utf8"),
  })));
  return { connection: child.stdio[3], ended };
}

test("the translator answers the question on the connection systemd hands it, and ends", async () => {
  const detect = started(SCRIPT, place());
  detect.connection.write("detect 15\nこんにちは");
  assert.deepEqual(await detect.ended, { status: 0, answer: "ok 2\nja", journal: "" });
  const translate = started(SCRIPT, place());
  translate.connection.write("translate ja en 15\nこんにちは");
  assert.deepEqual(await translate.ended, { status: 0, answer: "ok 17\n[こんにちは]", journal: "" });
  const none = started(SCRIPT, place());
  none.connection.write("translate ko en 15\n안녕하세요");
  assert.deepEqual(await none.ended, { status: 0, answer: "none\n", journal: "" });
});

/**
 * The translator started as systemd starts it, with the connection systemd
 * keeps as well: systemd lets go of it only once the translator has ended,
 * and counts it against `MaxConnections=1` until then. Here it lets go a
 * little after the end, so an answer whose end is read earlier ended
 * while the connection was still counted.
 */
async function heldBySystemd(script, directory) {
  const { directory: engineDirectory } = engine(directory);
  const socket = join(directory, "socket");
  const server = net.createServer({ pauseOnConnect: true });
  await new Promise((resolve) => server.listen(socket, resolve));
  const handed = new Promise((resolve) => server.once("connection", resolve));
  const portal = net.connect(socket);
  const kept = await handed;
  server.close();
  const child = spawn("sh", ["-c", 'LISTEN_PID=$$ LISTEN_FDS=1 exec "$0" "$@"', process.execPath, script,
    "--engine", engineDirectory, "--models", join(directory, "models"),
    "--fasttext", fasttext(directory, "__label__ja 0.97\n"), "--identifier", "/lid.176.ftz"],
  { stdio: ["ignore", "ignore", "ignore", kept] });
  let letGo = false;
  child.on("exit", () => setTimeout(() => {
    letGo = true;
    kept.destroy();
  }, 100));
  const said = [];
  portal.on("data", (chunk) => said.push(chunk));
  const read = new Promise((resolve) => portal.on("end", () => resolve({
    letGo,
    answer: Buffer.concat(said).toString("utf8"),
  })));
  return { portal, read };
}

test("the end of an answer is read only once systemd lets go of the connection, so the next question is let in", async () => {
  const { portal, read } = await heldBySystemd(SCRIPT, place());
  portal.write("detect 2\nhi");
  assert.deepEqual(await read, { letGo: true, answer: "ok 2\nja" });
});

test("a question it cannot answer gets no answer, and the journal says why and nothing of the text", async () => {
  const translator = started(SCRIPT, place());
  translator.connection.end("a secret of the writer's\n");
  assert.deepEqual(await translator.ended, {
    status: 1,
    answer: "",
    journal: "mooring-portal-translator: the question asks neither detect nor translate\n",
  });
});

test("the translator runs when it is started through a link to it", async () => {
  const directory = place();
  const link = join(directory, "translator.mjs");
  symlinkSync(SCRIPT, link);
  const translator = started(link, directory);
  translator.connection.write("detect 2\nhi");
  assert.equal((await translator.ended).answer, "ok 2\nja");
});

test("without the connection from systemd, or with a command line that lacks a place, it ends at once and says why", async () => {
  const lines = [];
  const log = (line) => lines.push(line);
  const flags = ["--engine", "/e", "--models", "/m", "--fasttext", "/f", "--identifier", "/i"];
  assert.equal(await run(flags, {}, 4242, log), 1);
  assert.equal(await run(flags, { LISTEN_FDS: "1", LISTEN_PID: "4241" }, 4242, log), 1);
  assert.equal(await run(flags, { LISTEN_FDS: "2", LISTEN_PID: "4242" }, 4242, log), 1);
  assert.equal(await run(flags.slice(0, 2), {}, 4242, log), 1);
  assert.equal(await run([...flags, "--verbose"], {}, 4242, log), 1);
  assert.equal(await run([...flags, "extra"], {}, 4242, log), 1);
  const handed = "mooring-portal-translator: systemd did not hand over one connection (LISTEN_FDS and LISTEN_PID)";
  assert.deepEqual(lines.slice(0, 4), [handed, handed, handed, "mooring-portal-translator: the command line lacks --models"]);
  assert.match(lines[4], /^mooring-portal-translator: the command line is wrong: .*--verbose/);
  assert.match(lines[5], /^mooring-portal-translator: the command line is wrong: .*extra/);
});

/** The lines of `files.txt`, read. */
function listed() {
  return FILES.split("\n")
    .filter((line) => line !== "" && !line.startsWith("#"))
    .map((line) => {
      const [sha256, path, size, url, ...rest] = line.split("  ");
      assert.deepEqual(rest, [], line);
      return { sha256, path, size, url };
    });
}

test("the notes start the translator with a command line it takes, where the listed files are", () => {
  const unit = NOTES.slice(NOTES.indexOf("`/etc/systemd/system/mooring-portal-translator@.service`"));
  const command = /\nExecStart=(.*)\n/.exec(unit)[1].split(" ");
  assert.equal(command[0], "/usr/bin/node");
  const options = optionsFrom(command.slice(2));
  // Debian's fastText, and the files the notes fetch into the directory
  // they fetch them in.
  assert.equal(options.fasttext, "/usr/bin/fasttext");
  const root = "/usr/local/lib/mooring-portal/translator";
  assert.ok(NOTES.includes(`\ncd ${root}\n`));
  assert.equal(options.engine, `${root}/engine`);
  assert.equal(options.models, `${root}/models`);
  const paths = listed().map(({ path }) => `${root}/${path}`);
  for (const file of [options.identifier, `${options.engine}/bergamot-translator.js`, `${options.engine}/bergamot-translator.wasm`]) {
    assert.ok(paths.includes(file), file);
  }
});

test("each listed file has its hash, size and address, and each listed model is one the translator takes", () => {
  const files = listed();
  assert.equal(files.length, 24);
  const models = place();
  const directions = new Map();
  for (const { sha256, path, size, url } of files) {
    assert.match(sha256, /^[0-9a-f]{64}$/, path);
    assert.match(size, /^[1-9][0-9]*$/, path);
    assert.match(url, /^https:\/\/(firefox-settings-attachments\.cdn\.mozilla\.net|raw\.githubusercontent\.com\/mozilla-firefox\/firefox\/[0-9a-f]{40}|dl\.fbaipublicfiles\.com)\//, path);
    assert.match(path, /^(engine\/[A-Za-z.-]+|lid\.176\.ftz|models\/(zh-Hant|[a-z]{2,3})-[a-z]{2,3}\/[0-9A-Za-z.-]+)$/, path);
    const direction = /^models\/(zh-Hant|[a-z]{2,3})-([a-z]{2,3})\/(.*)$/.exec(path);
    if (direction !== null) {
      model(models, direction[1], direction[2], [direction[3]]);
      directions.set(`${direction[1]}-${direction[2]}`, [direction[1], direction[2]]);
    }
  }
  assert.deepEqual([...directions.keys()].sort(), ["en-zh", "es-en", "ja-en", "ru-en", "zh-Hant-en", "zh-en"]);
  for (const [direction, [from, into]] of directions) {
    assert.ok(modelFiles(models, from, into).model, direction);
  }
});
