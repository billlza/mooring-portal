// The chat script on the operator's page of one conversation, against a
// stand-in page (`chat-stand.mjs`): the translations asked for with the
// messages, each written under its message as text, an answer that is
// not the one asked for, and a page that says wrongly where it stands.
// Run with `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { LONGEST, NEAR, start } from "../chat.mjs";
import { MARK, OPERATORS, WORDS, json, message, operatorPage, page, row, stand } from "./chat-stand.mjs";

test("the operator's page asks for the translations made after the last it shows, and writes each under its message", async () => {
  const chat = operatorPage({ after: "2", rows: [row(1), row(2)] });
  const site = stand(chat.root);
  start(site.win);
  const markup = "<img src=x onerror=alert(1)>\n第二行";
  site.answer(json(200, {
    messages: [message(3, { operator: false, body: "Hello" })],
    // Of a message the page showed already, and of the one that comes.
    translations: [{ id: 1, message: 1, body: "第一条的译文" }, { id: 4, message: 3, body: markup }],
  }));
  await site.elapse(NEAR);
  assert.equal(site.asked[0].path, `${OPERATORS}?after=2&translated=0`);
  const written = [`${MARK}|第一条的译文`, null, `${MARK}|${markup}`];
  assert.deepEqual(chat.thread.translations, written);
  // As the page writes one: under the message's own words, marked, and
  // as text.
  const line = chat.thread.children[2];
  assert.equal(line.dataset.id, "3");
  assert.deepEqual(line.children.map((part) => part.className), ["message-meta", "message-body", "message-translation"]);
  const [mark, words] = line.children[2].children;
  assert.equal(mark.className, "message-translated muted small");
  assert.equal(words.className, "message-body");
  assert.equal(words.children.length, 0, "text, not elements");
  // Then from the last of each.
  site.answer(json(200, { messages: [], translations: [] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[1].path, `${OPERATORS}?after=3&translated=4`);
  assert.deepEqual(chat.thread.translations, written);
  assert.equal(chat.status.hidden, true);
});

test("no translation is written twice, and another of a message takes the place of the one shown", async () => {
  const chat = operatorPage({ after: "1", translated: "3", rows: [row(1, { translation: "旧译文" })] });
  const site = stand(chat.root);
  start(site.win);
  site.answer(json(200, { messages: [], translations: [{ id: 3, message: 1, body: "已经显示过" }] }));
  await site.elapse(NEAR);
  assert.deepEqual(chat.thread.translations, [`${MARK}|旧译文`]);
  site.answer(json(200, {
    messages: [],
    translations: [
      { id: 2, message: 1, body: "更早的" },
      { id: 5, message: 1, body: "Written again" },
      { id: 5, message: 1, body: "And again" },
    ],
  }));
  await site.elapse(NEAR);
  assert.deepEqual(chat.thread.translations, [`${MARK}|Written again`]);
  assert.equal(chat.thread.children[0].children.length, 3);
  site.answer(json(200, { messages: [], translations: [] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[2].path, `${OPERATORS}?after=1&translated=5`);
});

test("a translation of a message the page does not show is written nowhere, and is not asked for again", async () => {
  const chat = operatorPage({ after: "1", rows: [row(1)] });
  const site = stand(chat.root);
  start(site.win);
  site.answer(json(200, { messages: [], translations: [{ id: 7, message: 9, body: "无处可写" }] }));
  await site.elapse(NEAR);
  assert.deepEqual(chat.thread.translations, [null]);
  assert.equal(chat.status.hidden, true);
  site.answer(json(200, { messages: [], translations: [] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[1].path, `${OPERATORS}?after=1&translated=7`);
});

test("an answer without the translations asked for, or with one that is none, is no answer", async () => {
  const chat = operatorPage({ after: "1", rows: [row(1)] });
  const site = stand(chat.root);
  start(site.win);
  const whole = { id: 1, message: 1, body: "译文" };
  const answers = [
    { messages: [message(2)] },
    { messages: [message(2)], translations: "none" },
    { messages: [message(2)], translations: [null] },
    { messages: [message(2)], translations: [{ ...whole, id: 0 }] },
    { messages: [message(2)], translations: [{ ...whole, id: 1.5 }] },
    { messages: [message(2)], translations: [{ ...whole, message: 0 }] },
    { messages: [message(2)], translations: [{ ...whole, message: "1" }] },
    { messages: [message(2)], translations: [{ ...whole, body: null }] },
    { messages: [message(2)], translations: [whole, { id: 2 }] },
  ];
  let wait = NEAR;
  for (const [index, answer] of answers.entries()) {
    site.answer(json(200, answer));
    await site.elapse(wait);
    // Nothing of it is written, its message neither.
    assert.equal(chat.status.textContent, WORDS["data-offline"], `${index}`);
    assert.equal(chat.thread.children.length, 1, `${index}`);
    assert.deepEqual(chat.thread.translations, [null], `${index}`);
    wait = Math.min(LONGEST, NEAR * 2 ** (index + 1));
  }
  // Asked again from where the page stands, a whole answer is taken.
  site.answer(json(200, { messages: [message(2)], translations: [whole] }));
  await site.elapse(wait);
  assert.equal(site.asked.at(-1).path, `${OPERATORS}?after=1&translated=0`);
  assert.equal(chat.status.hidden, true);
  assert.deepEqual(chat.thread.translations, [`${MARK}|译文`, null]);
});

test("a page that shows no translations asks for none, and writes none it is sent", async () => {
  const chat = page({ after: "1", rows: [row(1)] });
  const site = stand(chat.root);
  start(site.win);
  site.answer(json(200, { messages: [message(2)], translations: [{ id: 1, message: 1, body: "译文" }] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[0].path, "/feedback/messages?after=1");
  assert.equal(chat.thread.children.length, 2);
  assert.deepEqual(chat.thread.translations, [null, null]);
  assert.equal(chat.status.hidden, true);
});

test("a page that says wrongly where it stands among the translations, or not how to mark one, is a fault", () => {
  for (const [more, fault] of [
    [{ translated: "" }, /no data attribute translated/],
    [{ translated: "-1" }, /"-1" is not a translation number/],
    [{ translated: "1.5" }, /"1.5" is not a translation number/],
    [{ translated: "9007199254740993" }, /is not a translation number/],
    [{ "data-translation": undefined }, /no data attribute translation/],
    [{ "data-translation": "" }, /no data attribute translation/],
  ]) {
    const made = operatorPage(more);
    assert.throws(() => start(stand(made.root).win), fault, JSON.stringify(more));
  }
  // The page's place among the messages is named as that.
  assert.throws(() => start(stand(operatorPage({ after: "x" }).root).win), /"x" is not a message number/);
});
