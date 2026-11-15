// The chat script when things go wrong, against a stand-in page
// (`chat-stand.mjs`): a site that cannot be reached or answers what no
// request of the script is answered, a message that cannot be sent, a
// request turned away, and a page that lacks a part of the conversation.
// Run with `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { FAR, LONGEST, NEAR, start } from "../chat.mjs";
import { REDIRECTED, WORDS, box, empty, json, message, page, settle, stand, submit } from "./chat-stand.mjs";

test("when the site cannot be reached, the script says so and asks less often until it answers", async () => {
  const chat = page();
  const site = stand(chat.root);
  start(site.win);
  const failures = [
    Promise.reject(new TypeError("Failed to fetch")),
    empty(500),
    empty(502),
    Promise.resolve({ type: "basic", status: 200, json: () => Promise.reject(new SyntaxError("not JSON")) }),
    json(200, { messages: [{ id: 1 }] }),
    json(200, { messages: "none" }),
    json(200, { messages: [message(0)] }),
  ];
  let wait = NEAR;
  for (const [index, failure] of failures.entries()) {
    site.answer(failure);
    await site.elapse(wait);
    assert.equal(chat.status.hidden, false, `${index}`);
    assert.equal(chat.status.textContent, WORDS["data-offline"]);
    wait = Math.min(LONGEST, NEAR * 2 ** (index + 1));
  }
  assert.equal(wait, LONGEST);
  // An answer at last: it says no more, and asks as often as before.
  site.answer(json(200, { messages: [message(1)] }));
  await site.elapse(LONGEST);
  assert.equal(chat.status.hidden, true);
  assert.equal(chat.thread.children.length, 1);
  assert.deepEqual(site.waits(), [NEAR]);
});

test("the badge's question fails as the messages' do, and is asked less often after", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  site.answer(json(200, { unread: "yes" }));
  await site.elapse(FAR);
  assert.equal(dock.status.textContent, WORDS["data-offline"]);
  // Its usual wait doubled, never shorter than the usual.
  assert.deepEqual(site.waits(), [Math.min(LONGEST, FAR * 2)]);
  assert.ok(Math.min(LONGEST, FAR * 2) > FAR);
  site.answer(json(200, { unread: false }));
  await site.elapse(Math.min(LONGEST, FAR * 2));
  assert.equal(dock.status.hidden, true);
  assert.deepEqual(site.waits(), [FAR]);
});

test("a message that cannot be sent stays in the form, and the page says so until one is", async () => {
  const chat = page();
  const site = stand(chat.root);
  start(site.win);
  chat.field.value = "在吗";
  site.answer(Promise.reject(new TypeError("Failed to fetch")));
  await submit(chat.form);
  assert.equal(chat.field.value, "在吗");
  assert.equal(chat.status.textContent, WORDS["data-unsent"]);
  assert.equal(chat.button.disabled, false);
  // The questions answered after it do not take it back: the message is
  // still not sent, and nothing sends it again but the user.
  site.answer(json(200, { messages: [] }));
  await site.elapse(NEAR);
  assert.equal(chat.status.textContent, WORDS["data-unsent"]);
  // A question that fails says the site cannot be reached; once one is
  // answered, the page says again what became of the message.
  site.answer(empty(502));
  await site.elapse(NEAR);
  assert.equal(chat.status.textContent, WORDS["data-offline"]);
  site.answer(json(200, { messages: [] }));
  await site.elapse(NEAR * 2);
  assert.equal(chat.status.textContent, WORDS["data-unsent"]);
  assert.equal(site.unanswered(), 0);
  // Sent at last, it is said no more, nor after the site comes back from
  // another failure.
  site.answer(empty(204), json(200, { messages: [message(1, { mine: true, body: "在吗" })] }));
  await submit(chat.form);
  assert.equal(chat.status.hidden, true);
  assert.equal(chat.field.value, "");
  assert.equal(chat.thread.children.length, 1);
  site.answer(empty(502));
  await site.elapse(NEAR);
  assert.equal(chat.status.textContent, WORDS["data-offline"]);
  site.answer(json(200, { messages: [] }));
  await site.elapse(NEAR * 2);
  assert.equal(chat.status.hidden, true);
  assert.equal(chat.status.textContent, "");
});

test("a line the box says keeps the last message in sight, unless the reader scrolled back", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  dock.details.open = true;
  site.answer(json(200, { messages: [message(1), message(2), message(3)] }));
  dock.details.emit("toggle");
  await settle();
  const end = () => dock.thread.scrollHeight - dock.thread.clientHeight;
  const last = end();
  assert.equal(dock.thread.scrollTop, last);
  // The line takes its height from the conversation, which still ends in sight.
  site.answer(empty(502));
  await site.elapse(NEAR);
  assert.equal(dock.status.hidden, false);
  assert.ok(end() > last);
  assert.equal(dock.thread.scrollTop, end());
  // Scrolled back by the reader, it stays where it is.
  site.answer(json(200, { messages: [] }));
  await site.elapse(NEAR * 2);
  assert.equal(dock.status.hidden, true);
  dock.thread.scrollTop = 10;
  site.answer(empty(502));
  await site.elapse(NEAR);
  assert.equal(dock.status.hidden, false);
  assert.equal(dock.thread.scrollTop, 10);
});

for (const [name, turned] of [
  ["signed out", () => empty(401)],
  ["a stale token", () => empty(403)],
  ["a page that is gone", () => empty(404)],
  ["sent elsewhere", () => REDIRECTED],
]) {
  test(`turned away (${name}), the script says so, asks nothing more, and leaves the form to the browser`, async () => {
    const chat = page();
    const site = stand(chat.root);
    start(site.win);
    site.answer(turned());
    await site.elapse(NEAR);
    assert.equal(chat.status.textContent, WORDS["data-expired"]);
    assert.deepEqual(site.waits(), []);
    await site.show();
    assert.equal(site.asked.length, 1);
    // The form posts the page's way now.
    chat.field.value = "你好";
    assert.equal(await submit(chat.form), false);
    assert.equal(site.asked.length, 1);
  });
}

test("a message turned away ends the script too", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  dock.field.value = "你好";
  site.answer(empty(403));
  assert.equal(await submit(dock.form), true);
  assert.equal(dock.status.textContent, WORDS["data-expired"]);
  assert.deepEqual(site.waits(), []);
  dock.details.open = true;
  dock.details.emit("toggle");
  await settle();
  assert.equal(site.asked.length, 1);
  site.answer(empty(401));
  await site.hide();
  assert.equal(await submit(dock.form), false);
  assert.equal(site.unanswered(), 1);
});

test("the badge's question turned away ends the script", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  site.answer(REDIRECTED);
  await site.elapse(FAR);
  assert.equal(dock.status.textContent, WORDS["data-expired"]);
  assert.deepEqual(site.waits(), []);
});

test("a page that lacks a part of the conversation, or says it wrongly, is a fault", () => {
  const missing = [
    () => page({ "data-chat": "chat" }),
    () => page({ after: "-1" }),
    () => page({ after: "01" }),
    () => page({ after: "9007199254740993" }),
    () => page({ "data-expired": "" }),
    () => page({ "data-unsent": undefined }),
    () => box({ "data-unread": undefined }),
  ];
  for (const make of missing) {
    const made = make();
    const site = stand(made.root ?? made.details);
    assert.throws(() => start(site.win), /conversation|message number|box or a page/);
  }
  // Each part, in turn.
  for (const part of ["thread", "empty", "status", "form", "field", "button"]) {
    const made = page();
    const node = made[part];
    node.remove();
    assert.throws(() => start(stand(made.root).win), /the conversation has no/, part);
  }
  // A form without its token, or with an empty one.
  for (const spoil of [
    (form) => (form.elements = form.elements.filter((element) => element.name !== "csrf")),
    (form) => (form.elements[0].value = ""),
  ]) {
    const made = page();
    spoil(made.form);
    assert.throws(() => start(stand(made.root).win), /carries no token/);
  }
  // A box that is in no details element.
  const loose = box();
  loose.panel.remove();
  assert.throws(() => start(stand(loose.panel).win), /not in a details element/);
  // Nor in one without its summary.
  const bare = box();
  bare.summary.remove();
  assert.throws(() => start(stand(bare.details).win), /the conversation has no summary/);
});
