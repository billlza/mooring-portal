// The chat script while the site answers, against a stand-in page
// (`chat-stand.mjs`): asking while in sight, the badge, the box opened
// and closed, a hidden page, and messages sent or refused. Run with
// `node --test "crates/web/assets/js/test/*.test.mjs"`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { FAR, NEAR, start } from "../chat.mjs";
import { TOKEN, WORDS, box, empty, json, message, page, settle, stand, submit } from "./chat-stand.mjs";

test("a page asks for what came after the last message it shows, while in sight", async () => {
  const chat = page({ after: "4" });
  const site = stand(chat.root);
  start(site.win);
  site.answer(json(200, { messages: [message(5), message(6, { mine: true })] }));
  await site.elapse(NEAR);
  assert.deepEqual(site.asked.map((asked) => asked.path), ["/feedback/messages?after=4"]);
  assert.deepEqual(site.asked[0].init, {
    headers: { "X-CSRF-Token": TOKEN },
    cache: "no-store",
    redirect: "manual",
  });
  assert.deepEqual(chat.thread.messages, [
    "运营者|2026-11-05 20:00|第 5 条|message from-operator",
    "你|2026-11-05 20:00|第 6 条|message mine",
  ]);
  assert.equal(chat.thread.hidden, false);
  assert.equal(chat.empty.hidden, true);
  // Then from the last one shown, again and again.
  site.answer(json(200, { messages: [] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[1].path, "/feedback/messages?after=6");
  assert.equal(chat.thread.children.length, 2);
  assert.deepEqual(site.waits(), [NEAR]);
});

test("what the site sends is written as text, and nothing is shown twice", async () => {
  const chat = page({ after: "1", messages: "/operator/feedback/abcd/messages" });
  const site = stand(chat.root);
  start(site.win);
  const markup = "<img src=x onerror=alert(1)>\n第二行";
  site.answer(json(200, { messages: [message(1), message(2, { body: markup }), message(2), message(3, { mine: true, operator: true })] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[0].path, "/operator/feedback/abcd/messages?after=1");
  assert.deepEqual(chat.thread.messages, [
    `运营者|2026-11-05 20:00|${markup}|message from-operator`,
    "你|2026-11-05 20:00|第 3 条|message mine from-operator",
  ]);
  assert.equal(chat.thread.children[0].children[1].children.length, 0, "text, not elements");
});

test("a closed box asks whether a reply waits, and shows the badge or takes it away", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  site.answer(json(200, { unread: true }));
  await site.elapse(FAR);
  assert.deepEqual(site.asked.map((asked) => asked.path), ["/feedback/unread"]);
  assert.equal(dock.summary.querySelectorAll(".dock-unread").length, 1);
  assert.equal(dock.summary.querySelector(".dock-unread").textContent, "有新回复");
  // Asked again, it stays one badge.
  site.answer(json(200, { unread: true }));
  await site.elapse(FAR);
  assert.equal(dock.summary.querySelectorAll(".dock-unread").length, 1);
  site.answer(json(200, { unread: false }));
  await site.elapse(FAR);
  assert.equal(dock.summary.querySelector(".dock-unread"), null);
  // Nothing is shown of the conversation until the box is opened.
  assert.equal(dock.thread.hidden, true);
  assert.equal(dock.empty.hidden, true);
});

test("opening the box shows the whole conversation at once and reads it", async () => {
  const dock = box({ unread: true });
  const site = stand(dock.details);
  start(site.win);
  assert.deepEqual(site.waits(), [FAR]);
  site.answer(json(200, { messages: [message(1, { mine: true }), message(2)] }));
  dock.details.open = true;
  dock.details.emit("toggle");
  await settle();
  assert.deepEqual(site.asked.map((asked) => asked.path), ["/feedback/messages?after=0"]);
  assert.equal(dock.thread.children.length, 2);
  assert.equal(dock.thread.hidden, false);
  assert.equal(dock.empty.hidden, true);
  assert.equal(dock.summary.querySelector(".dock-unread"), null, "read now");
  assert.ok(dock.thread.scrollTop > 0, "the conversation is longer than the box");
  assert.equal(dock.thread.scrollTop, dock.thread.scrollHeight - dock.thread.clientHeight);
  // Open, it asks often; closed again, now and then, and for the badge.
  site.answer(json(200, { messages: [message(3)] }));
  await site.elapse(NEAR);
  assert.equal(site.asked[1].path, "/feedback/messages?after=2");
  dock.details.open = false;
  dock.details.emit("toggle");
  assert.deepEqual(site.waits(), [FAR]);
  site.answer(json(200, { unread: false }));
  await site.elapse(FAR);
  assert.equal(site.asked[2].path, "/feedback/unread");
});

test("an empty conversation says so", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  site.answer(json(200, { messages: [] }));
  dock.details.open = true;
  dock.details.emit("toggle");
  await settle();
  assert.equal(dock.thread.hidden, true);
  assert.equal(dock.empty.hidden, false);
});

test("nothing is asked while the page is hidden, and showing it asks at once", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  await site.hide();
  assert.deepEqual(site.waits(), []);
  // A box opened behind a hidden page asks nothing, nor does closing it.
  dock.details.open = true;
  dock.details.emit("toggle");
  await settle();
  assert.equal(site.asked.length, 0);
  site.answer(json(200, { messages: [message(1)] }));
  await site.show();
  assert.deepEqual(site.asked.map((asked) => asked.path), ["/feedback/messages?after=0"]);
  assert.deepEqual(site.waits(), [NEAR]);
  await site.hide();
  dock.details.open = false;
  dock.details.emit("toggle");
  assert.deepEqual(site.waits(), []);
});

test("a page hidden from the start asks nothing until shown", async () => {
  const chat = page();
  const site = stand(chat.root);
  site.win.document.visibilityState = "hidden";
  start(site.win);
  assert.deepEqual(site.waits(), []);
  site.answer(json(200, { messages: [] }));
  await site.show();
  assert.equal(site.asked.length, 1);
});

test("a message is sent as the form would send it, and comes back with the rest", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  dock.details.open = true;
  site.answer(json(200, { messages: [] }));
  dock.details.emit("toggle");
  await settle();
  dock.field.value = "链接打不开 & <b>";
  site.answer(empty(204), json(200, { messages: [message(1, { mine: true, body: "链接打不开 & <b>" })] }));
  assert.equal(await submit(dock.form), true, "the page stays");
  const sent = site.asked[1];
  assert.equal(sent.path, "/feedback/messages");
  assert.equal(sent.init.method, "POST");
  assert.equal(sent.init.redirect, "manual");
  assert.equal(String(sent.init.body), `csrf=${TOKEN}&body=%E9%93%BE%E6%8E%A5%E6%89%93%E4%B8%8D%E5%BC%80+%26+%3Cb%3E`);
  // The field is emptied, and what was sent is asked for at once.
  assert.equal(dock.field.value, "");
  assert.equal(site.asked[2].path, "/feedback/messages?after=0");
  assert.deepEqual(dock.thread.messages, ["你|2026-11-05 20:00|链接打不开 & <b>|message mine"]);
  assert.equal(dock.button.disabled, false);
  assert.equal(dock.status.hidden, true);
});

test("a message the site refuses stays in the form, with the site's reason", async () => {
  const chat = page();
  const site = stand(chat.root);
  start(site.win);
  for (const [status, reason] of [[422, "消息太长了，最多 1000 字。"], [429, "24 小时内最多发 20 条消息，请稍后再发。"]]) {
    chat.field.value = "太长的消息";
    site.answer(json(status, { error: reason }));
    assert.equal(await submit(chat.form), true);
    assert.equal(chat.field.value, "太长的消息");
    assert.equal(chat.status.hidden, false);
    assert.equal(chat.status.textContent, reason);
    assert.equal(chat.button.disabled, false);
  }
  // A refusal without a reason is no answer this script knows: the
  // message is not sent, and the page says so.
  site.answer(json(422, { problem: "x" }));
  await submit(chat.form);
  assert.equal(chat.status.textContent, WORDS["data-unsent"]);
  // The next message sent clears what was said.
  site.answer(empty(204), json(200, { messages: [] }));
  await submit(chat.form);
  assert.equal(chat.status.hidden, true);
  assert.equal(chat.status.textContent, "");
});

test("one message is sent at a time", async () => {
  const chat = page();
  const site = stand(chat.root);
  start(site.win);
  let arrive;
  site.answer(() => new Promise((resolve) => (arrive = resolve)));
  chat.field.value = "一条";
  await submit(chat.form);
  assert.equal(chat.button.disabled, true);
  // Sent again while the first is out: the page stays, nothing is sent.
  assert.equal(await submit(chat.form), true);
  assert.equal(site.asked.length, 1);
  site.answer(json(200, { messages: [] }));
  arrive({ type: "basic", status: 204 });
  await settle();
  assert.equal(chat.button.disabled, false);
  assert.equal(site.asked.length, 2);
});

test("one question at a time: one asked for while another is out follows it", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  let arrive;
  site.answer(() => new Promise((resolve) => (arrive = resolve)));
  await site.elapse(FAR);
  assert.equal(site.asked[0].path, "/feedback/unread");
  // Opened while the badge's question is out: the messages are asked for
  // once it is answered, not alongside it.
  dock.details.open = true;
  dock.details.emit("toggle");
  await settle();
  assert.equal(site.asked.length, 1);
  site.answer(json(200, { messages: [message(1)] }));
  arrive({ type: "basic", status: 200, json: () => Promise.resolve({ unread: true }) });
  await settle();
  assert.deepEqual(site.asked.map((asked) => asked.path), ["/feedback/unread", "/feedback/messages?after=0"]);
  assert.equal(dock.thread.children.length, 1);
  assert.equal(dock.summary.querySelector(".dock-unread"), null);
  assert.deepEqual(site.waits(), [NEAR]);
});

test("every question carries the token of the page's form", async () => {
  const dock = box();
  const site = stand(dock.details);
  start(site.win);
  site.answer(json(200, { unread: false }));
  await site.elapse(FAR);
  dock.details.open = true;
  site.answer(json(200, { messages: [] }));
  dock.details.emit("toggle");
  await settle();
  dock.field.value = "你好";
  site.answer(empty(204), json(200, { messages: [] }));
  await submit(dock.form);
  assert.deepEqual(site.asked.map((asked) => asked.path), [
    "/feedback/unread",
    "/feedback/messages?after=0",
    "/feedback/messages",
    "/feedback/messages?after=0",
  ]);
  for (const asked of site.asked) {
    assert.deepEqual(asked.init.headers, { "X-CSRF-Token": TOKEN }, asked.path);
  }
});

test("every conversation on the page is kept, each its own way", async () => {
  const dock = box();
  const chat = page({ after: "2" });
  const site = stand(dock.details, chat.root);
  start(site.win);
  assert.deepEqual(site.waits().sort((a, b) => a - b), [NEAR, FAR]);
});
