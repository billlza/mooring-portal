// A stand-in page for the tests of the chat script: the conversation's
// elements as `base.html` and `thread.html` write them, a site that
// answers as the test says, and a clock the test turns. No browser is
// needed.

import assert from "node:assert/strict";

class Node {
  constructor(tag, attributes = {}) {
    this.tag = tag;
    this.children = [];
    this.parent = null;
    this.dataset = {};
    this.className = "";
    this.hidden = false;
    this.handlers = new Map();
    this.text = "";
    for (const [name, value] of Object.entries(attributes)) {
      if (name === "class") {
        this.className = value;
      } else if (name.startsWith("data-")) {
        const key = name.slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
        this.dataset[key] = value;
      } else {
        this[name] = value;
      }
    }
  }

  set textContent(text) {
    this.children = [];
    this.text = String(text);
  }

  get textContent() {
    return this.text + this.children.map((child) => child.textContent).join("");
  }

  append(...nodes) {
    for (const node of nodes) {
      const child = typeof node === "string" ? Object.assign(new Node("#text"), { text: node }) : node;
      child.parent = this;
      this.children.push(child);
    }
  }

  remove() {
    this.parent.children = this.parent.children.filter((child) => child !== this);
    this.parent = null;
  }

  matches(selector) {
    if (selector.startsWith("[data-")) {
      const key = selector.slice(6, -1).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
      return key in this.dataset;
    }
    if (selector.startsWith(".")) {
      return this.className.split(" ").includes(selector.slice(1));
    }
    return this.tag === selector;
  }

  *walk() {
    for (const child of this.children) {
      yield child;
      yield* child.walk();
    }
  }

  querySelector(selector) {
    return this.querySelectorAll(selector)[0] ?? null;
  }

  querySelectorAll(selector) {
    return [...this.walk()].filter((node) => node.matches(selector));
  }

  closest(selector) {
    let node = this;
    while (node && !node.matches(selector)) {
      node = node.parent;
    }
    return node;
  }

  addEventListener(type, handler) {
    this.handlers.set(type, [...(this.handlers.get(type) ?? []), handler]);
  }

  emit(type, event = {}) {
    for (const handler of this.handlers.get(type) ?? []) {
      handler(event);
    }
    return event;
  }

  /** Under each message shown, its translation as `mark|text`, or `null`. */
  get translations() {
    return this.children.map((line) => {
      const blocks = line.children.filter((part) => part.className === "message-translation");
      assert.ok(blocks.length <= 1, "at most one translation under a message");
      return blocks.length === 1 ? blocks[0].children.map((part) => part.textContent).join("|") : null;
    });
  }

  /** The texts of the messages shown, as `who|sent|body|classes`. */
  get messages() {
    return this.children.map((line) => {
      const [meta, body] = line.children;
      const [who, , sent] = meta.children;
      return `${who.textContent}|${sent.textContent}|${body.textContent}|${line.className}`;
    });
  }
}

/** The words the page gives the script. */
export const WORDS = {
  "data-expired": "页面已过期，请刷新后再继续。",
  "data-offline": "暂时连不上网站，稍后会自动重试。",
  "data-unsent": "没有发出去：暂时连不上网站。文字还在框里，稍后再按一次发送。",
};

/** What marks a translation on the page. */
export const MARK = "机器翻译，可能有误";

/** The token the page's form carries. */
export const TOKEN = "a".repeat(64);

/**
 * Makes `list` scroll as a browser's does: each message takes 60 of its
 * height, it shows 100, or 70 while `status` is shown, and `scrollTop`
 * stays between its top and its end.
 */
function scrolling(list, status) {
  let top = 0;
  const height = () => 60 * list.children.length;
  const shown = () => (status.hidden ? 100 : 70);
  const end = () => Math.max(0, height() - shown());
  Object.defineProperties(list, {
    scrollHeight: { get: height },
    clientHeight: { get: shown },
    scrollTop: {
      get: () => (top = Math.min(top, end())),
      set: (value) => (top = Math.max(0, Math.min(value, end()))),
    },
  });
}

/** The parts of a conversation, in `root`, with a form posting `fields`. */
function parts(root, { empty, thread, rows = [] }) {
  const emptyNote = new Node("p", { class: "muted", "data-chat-empty": "", hidden: empty });
  emptyNote.textContent = "还没有消息。";
  const list = new Node("ol", { class: "thread", "data-chat-thread": "", hidden: thread });
  for (const row of rows) {
    list.append(row);
  }
  const form = new Node("form");
  const csrf = new Node("input", { name: "csrf", value: TOKEN });
  const field = new Node("textarea", { name: "body", value: "" });
  const button = new Node("button", { disabled: false });
  form.append(csrf, field, button);
  form.elements = [csrf, field, button];
  const status = new Node("p", { class: "small chat-status", "data-chat-status": "", hidden: true });
  scrolling(list, status);
  root.append(emptyNote, list, form, status);
  return { empty: emptyNote, thread: list, form, field, button, status };
}

/** The box at the bottom right, closed, with or without the badge. */
export function box({ unread = false, ...more } = {}) {
  const details = new Node("details", { class: "dock", open: false });
  const summary = new Node("summary");
  summary.append(new Node("span"));
  if (unread) {
    const mark = new Node("span", { class: "dock-unread" });
    mark.textContent = "有新回复";
    summary.append(mark);
  }
  const panel = new Node("div", {
    class: "dock-panel",
    "data-chat": "box",
    "data-messages": "/feedback/messages",
    "data-unread": "/feedback/unread",
    "data-unread-text": "有新回复",
    ...WORDS,
    ...more,
  });
  details.append(summary, panel);
  return { details, summary, panel, ...parts(panel, { empty: true, thread: true }) };
}

/** A conversation page that shows the messages up to number `after`. */
export function page({ after = "0", messages = "/feedback/messages", rows = [], ...more } = {}) {
  const root = new Node("div", {
    class: "chat",
    "data-chat": "page",
    "data-messages": messages,
    "data-after": after,
    ...WORDS,
    ...more,
  });
  return { root, ...parts(root, { empty: rows.length > 0, thread: rows.length === 0, rows }) };
}

/** Where the script of the operator's page of a conversation asks. */
export const OPERATORS = "/operator/feedback/abcd/messages";

/**
 * The operator's page of one conversation, which shows the translations
 * up to number `translated` (`operator_conversation.html`).
 */
export function operatorPage({ translated = "0", ...more } = {}) {
  return page({ messages: OPERATORS, "data-translated": translated, "data-translation": MARK, ...more });
}

/**
 * Message number `id` as `thread.html` writes it into the page, with
 * `translation` under it where the page shows one.
 */
export function row(id, { translation = null } = {}) {
  const line = new Node("li", { class: "message", "data-id": String(id) });
  const meta = new Node("p", { class: "message-meta" });
  const who = new Node("span", { class: "message-who" });
  who.textContent = "abcd";
  const sent = new Node("span", { class: "muted small" });
  sent.textContent = "2026-11-05 20:00";
  meta.append(who, " ", sent);
  const body = new Node("p", { class: "message-body" });
  body.textContent = `第 ${id} 条`;
  line.append(meta, body);
  if (translation !== null) {
    const block = new Node("div", { class: "message-translation" });
    const mark = new Node("p", { class: "message-translated muted small" });
    mark.textContent = MARK;
    const words = new Node("p", { class: "message-body" });
    words.textContent = translation;
    block.append(mark, words);
    line.append(block);
  }
  return line;
}

/** A site that answers as the test says, and a clock the test turns. */
export function stand(...roots) {
  const body = new Node("body");
  body.append(...roots);
  const document = new Node("#document");
  document.visibilityState = "visible";
  document.append(body);
  document.createElement = (tag) => new Node(tag);
  const asked = [];
  const answers = [];
  const timers = new Map();
  let next = 1;
  const win = {
    document,
    URLSearchParams,
    fetch(path, init) {
      asked.push({ path, init });
      const answer = answers.shift();
      assert.ok(answer, `nothing to answer ${path}`);
      return typeof answer === "function" ? answer() : answer;
    },
    setTimeout(handler, wait) {
      timers.set(next, { handler, wait });
      next += 1;
      return next - 1;
    },
    clearTimeout(id) {
      timers.delete(id);
    },
  };
  return {
    win,
    asked,
    /** What the site answers next. */
    answer(...more) {
      answers.push(...more);
    },
    /** The waits of the timers set, in the order set. */
    waits() {
      return [...timers.values()].map((timer) => timer.wait);
    },
    /** Runs the timer that is set, which must wait `wait`, and lets the answer arrive. */
    async elapse(wait) {
      assert.deepEqual(this.waits(), [wait]);
      const [[id, timer]] = timers;
      timers.delete(id);
      timer.handler();
      await settle();
    },
    async hide() {
      document.visibilityState = "hidden";
      document.emit("visibilitychange");
      await settle();
    },
    async show() {
      document.visibilityState = "visible";
      document.emit("visibilitychange");
      await settle();
    },
    unanswered() {
      return answers.length;
    },
  };
}

/** Lets every answer that has come be read. */
export async function settle() {
  for (let round = 0; round < 10; round += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

/** An answer of `status` with `body` as its JSON. */
export function json(status, body) {
  return Promise.resolve({ type: "basic", status, json: () => Promise.resolve(body) });
}

/** An answer of `status` with no body. */
export function empty(status) {
  return Promise.resolve({
    type: "basic",
    status,
    json: () => Promise.reject(new SyntaxError("no body")),
  });
}

/** The answer to a request the site sent elsewhere. */
export const REDIRECTED = Promise.resolve({ type: "opaqueredirect", status: 0 });

/** Message number `id` as the site sends it. */
export function message(id, { mine = false, operator = !mine, body = `第 ${id} 条` } = {}) {
  return { id, mine, from_operator: operator, who: mine ? "你" : "运营者", sent: "2026-11-05 20:00", body };
}

/** Submits `form`; whether the script kept the page. */
export async function submit(form) {
  let prevented = false;
  form.emit("submit", { preventDefault: () => (prevented = true) });
  await settle();
  return prevented;
}
