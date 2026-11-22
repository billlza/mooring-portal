// The translator's questions and answers (docs/translation.md, section 2):
// what a question may be, how its bytes are read as they come, how an
// answer is written, and what the portal reads on the connection when
// the translator answers, and when it cannot.
// Run with `node --test "crates/portal/translator/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  Failure,
  MAX_LINE_BYTES,
  MAX_TEXT_BYTES,
  NOTHING,
  QuestionReader,
  answerOne,
  answerTo,
  parseLine,
  said,
} from "../translator.mjs";
import { connection, place } from "./stand.mjs";

/** The failure `call` throws, by its words. */
function refusal(call) {
  try {
    call();
  } catch (error) {
    assert.ok(error instanceof Failure, String(error));
    return error.message;
  }
  assert.fail("no failure");
}

test("a question says what it asks and how many bytes of text follow, and nothing else is a question", () => {
  assert.deepEqual(parseLine("detect 5"), { ask: "detect", length: 5 });
  assert.deepEqual(parseLine("translate ja en 12"), { ask: "translate", from: "ja", into: "en", length: 12 });
  assert.deepEqual(parseLine("translate yue zh 1"), { ask: "translate", from: "yue", into: "zh", length: 1 });
  assert.deepEqual(parseLine(`detect ${MAX_TEXT_BYTES}`), { ask: "detect", length: MAX_TEXT_BYTES });
  const neither = "the question asks neither detect nor translate";
  for (const line of ["", "detect", "Detect 5", "detect  5", "detect\t5", "detect 5 6", "translate ja 5",
    "translate ja en", "translate JA en 5", "translate japan en 5", "translate j en 5", "translate ja e1 5",
    "translate ja en zh 5", "ask ja en 5"]) {
    // Its last word is taken for the length: without one, what is left
    // asks nothing.
    assert.equal(refusal(() => parseLine(line)), neither, line);
  }
  for (const line of ["detect 0", "detect 05", "detect -5", "detect +5", "detect 5.0", "detect 1e3", "detect 5\r", "detect ５"]) {
    assert.equal(refusal(() => parseLine(line)), "the question does not say in digits how many bytes of text follow", line);
  }
  assert.equal(refusal(() => parseLine(`detect ${MAX_TEXT_BYTES + 1}`)), "the question's text is longer than any the portal asks about");
});

test("a question is read however its bytes come, and only once all of them have come", () => {
  const text = Buffer.from("こんにちは\n二行目", "utf8");
  const bytes = Buffer.concat([Buffer.from(`translate ja en ${text.length}\n`), text]);
  // One byte at a time: the first line, then a character, come in pieces.
  const reader = new QuestionReader();
  for (const byte of bytes.subarray(0, -1)) {
    assert.equal(reader.push(Buffer.from([byte])), null);
  }
  assert.deepEqual(reader.push(bytes.subarray(-1)), { ask: "translate", from: "ja", into: "en", length: text.length, text: "こんにちは\n二行目" });
  // All at once.
  assert.deepEqual(new QuestionReader().push(Buffer.from("detect 2\nhi")), { ask: "detect", length: 2, text: "hi" });
  // A byte order mark is the text's, as the portal sent it.
  assert.equal(new QuestionReader().push(Buffer.from("detect 5\n﻿hi", "utf8")).text, "﻿hi");
});

test("bytes that cannot be a question are refused as soon as they come", () => {
  const long = "the question's first line is too long";
  assert.equal(new QuestionReader().push(Buffer.from("d".repeat(MAX_LINE_BYTES))), null);
  assert.equal(refusal(() => new QuestionReader().push(Buffer.from("d".repeat(MAX_LINE_BYTES + 1)))), long);
  assert.equal(refusal(() => new QuestionReader().push(Buffer.from(`${"d".repeat(MAX_LINE_BYTES + 1)}\n`))), long);
  const reader = new QuestionReader();
  reader.push(Buffer.from("d".repeat(MAX_LINE_BYTES - 1)));
  assert.equal(refusal(() => reader.push(Buffer.from("dd"))), long);
  // A line of bytes that are not ASCII asks nothing.
  assert.equal(refusal(() => new QuestionReader().push(Buffer.from("détect 2\nhi", "utf8"))), "the question asks neither detect nor translate");
  // More text than it says, at once or later.
  assert.equal(refusal(() => new QuestionReader().push(Buffer.from("detect 2\nhi!"))), "more text came than the question says");
  const later = new QuestionReader();
  assert.equal(later.push(Buffer.from("detect 3\nh")), null);
  assert.equal(refusal(() => later.push(Buffer.from("i!!"))), "more text came than the question says");
  // Text that is not UTF-8: a lone continuation byte, a cut character, an
  // encoded surrogate.
  for (const text of [[0x80], [0xe3, 0x81], [0xed, 0xa0, 0x80]]) {
    const question = Buffer.concat([Buffer.from(`detect ${text.length}\n`), Buffer.from(text)]);
    assert.equal(refusal(() => new QuestionReader().push(question)), "the question's text is not UTF-8");
  }
});

test("an answer says how many bytes follow it, not how many characters", () => {
  assert.equal(said("ja").toString("utf8"), "ok 2\nja");
  assert.equal(said("你好").toString("utf8"), "ok 6\n你好");
  assert.equal(said("").toString("utf8"), "ok 0\n");
  assert.equal(NOTHING.toString("utf8"), "none\n");
});

test("a question is answered by what it asks for, and by nothing when there is nothing to say", async () => {
  const asked = [];
  const tools = {
    identify: async (text) => (asked.push(["identify", text]), text === "?" ? null : "ja"),
    translate: async (from, into, text) => (asked.push(["translate", from, into, text]), from === "ko" ? null : "Hello"),
  };
  assert.equal((await answerTo({ ask: "detect", text: "こんにちは" }, tools)).toString(), "ok 2\nja");
  assert.equal((await answerTo({ ask: "detect", text: "?" }, tools)).toString(), "none\n");
  assert.equal((await answerTo({ ask: "translate", from: "ja", into: "en", text: "こんにちは" }, tools)).toString(), "ok 5\nHello");
  assert.equal((await answerTo({ ask: "translate", from: "ko", into: "en", text: "안녕" }, tools)).toString(), "none\n");
  assert.deepEqual(asked, [
    ["identify", "こんにちは"],
    ["identify", "?"],
    ["translate", "ja", "en", "こんにちは"],
    ["translate", "ko", "en", "안녕"],
  ]);
});

/** Tools that answer and write down what they were asked. */
function writingDown() {
  const asked = [];
  return {
    asked,
    tools: {
      identify: async (text) => (asked.push(text), "zh"),
      translate: async (from, into, text) => (asked.push(`${from} ${into} ${text}`), "Hello, world"),
    },
  };
}

test("the translator reads the question on its connection, answers there and ends the connection", async () => {
  const directory = place();
  const { translator, portal, answer } = await connection(directory);
  const { asked, tools } = writingDown();
  const answered = answerOne(translator, tools);
  const text = Buffer.from("你好，世界", "utf8");
  // The portal writes its line, then the text, and keeps its side open.
  portal.write(`translate zh en ${text.length}\n`);
  portal.write(text.subarray(0, 4));
  portal.write(text.subarray(4));
  await answered;
  assert.equal((await answer).toString("utf8"), "ok 12\nHello, world");
  assert.deepEqual(asked, ["zh en 你好，世界"]);
  assert.ok(translator.destroyed);
});

test("a connection that ends before its question does, or does not hold one, gets no answer", async () => {
  for (const [bytes, reason] of [
    ["detect 10\nshort", "the connection ended before the question did"],
    ["", "the connection ended before the question did"],
    ["hello there, translator\n", "the question asks neither detect nor translate"],
    ["detect 2\nhi and more", "more text came than the question says"],
  ]) {
    const { translator, portal, answer } = await connection(place());
    const { asked, tools } = writingDown();
    const answered = answerOne(translator, tools);
    portal.end(bytes);
    await assert.rejects(answered, (error) => error instanceof Failure && error.message === reason);
    assert.equal((await answer).length, 0, bytes);
    assert.deepEqual(asked, []);
  }
});

test("a question whose tool fails gets no answer, and the failure says nothing of the text", async () => {
  const { translator, portal, answer } = await connection(place());
  const answered = answerOne(translator, {
    identify: async () => {
      throw new Failure("fastText ended with 1");
    },
    translate: async () => assert.fail("not asked"),
  });
  portal.write("detect 6\nsecret");
  await assert.rejects(answered, (error) => error.message === "fastText ended with 1");
  assert.equal((await answer).length, 0);
});
