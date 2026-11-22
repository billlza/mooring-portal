// The conversation with the operator, kept current while a page shows it:
// in the box at the bottom right of a signed-in user's pages, on the
// feedback page, and on the operator's page of one conversation. Without
// this script each of them works as a plain form (`base.html`), and so do
// they when the site turns the script away.
//
// While a conversation is in sight, the script asks this site every few
// seconds for the messages after the last one it shows, and writes them
// into the page as text. A closed box asks now and then whether a reply
// waits, and nothing is asked while the page is hidden. A message is sent
// as the page's form would send it, without leaving the page, and comes
// back with the next answer; one that cannot be sent stays in the form,
// and the page says so until the next is sent. Every request carries the
// token of the page's form, which the site accepts only while it is the
// browser's: once anyone signs in or out in this browser, a page shown
// before asks nothing more. When the site turns a request away (the
// session has ended, the token is stale, the page is gone), the script
// says so, asks nothing more, and leaves the form to the browser. When
// the site cannot be reached, it says so and asks again, less often.
//
// The operator's page shows, under a message a user wrote in another
// language, its translation into the page's. A translation is made after
// its message, so with the messages the script of that page asks for the
// translations made after the last it shows, and writes each under its
// message, as text.
//
// The page names what to ask for and what to say on the conversation's
// element, `data-chat`, and holds its parts (`data-chat-thread`, ...).
// Everything is reached through the window handed to `start`, so the
// whole of it can be run against a stand-in (`test/chat-stand.mjs`).

/** How often a conversation in sight asks for new messages, in ms. */
export const NEAR = 3000;
/** How often a closed box asks whether a reply waits. */
export const FAR = 30000;
/** The longest wait after the site could not be reached. */
export const LONGEST = 60000;

/** The header that carries the token of the page's form. */
const TOKEN_HEADER = "X-CSRF-Token";
/** What the site answers when it turns a request away. */
const TURNED_AWAY = new Set([401, 403, 404]);
/** What it answers when it refuses a message, with the reason. */
const REFUSED = new Set([422, 429]);

/** Starts every conversation of the page in `win`. */
export function start(win) {
  for (const root of win.document.querySelectorAll("[data-chat]")) {
    converse(win, root);
  }
}

function need(root, selector) {
  const found = root.querySelector(selector);
  if (!found) {
    throw new Error(`the conversation has no ${selector}`);
  }
  return found;
}

function word(root, name) {
  const value = root.dataset[name];
  if (typeof value !== "string" || value === "") {
    throw new Error(`the conversation has no data attribute ${name}`);
  }
  return value;
}

/** The token the conversation's form carries. */
function tokenOf(form) {
  for (const element of form.elements) {
    if (element.name === "csrf" && typeof element.value === "string" && element.value !== "") {
      return element.value;
    }
  }
  throw new Error("the conversation's form carries no token");
}

/**
 * The number the page wrote of the last `what` it shows (a message, a
 * translation): 0, or a whole number above it.
 */
function number(text, what) {
  if (!/^(0|[1-9][0-9]*)$/.test(text) || !Number.isSafeInteger(Number(text))) {
    throw new Error(`${JSON.stringify(text)} is not a ${what} number`);
  }
  return Number(text);
}

/**
 * What the site said to a request carrying `token`: `done`, with what it
 * sent; `refused`, with why; `away` when it turned the request away;
 * `failed` when it could not be reached or answered what no request of
 * this script is answered.
 */
async function ask(win, path, token, init) {
  const headers = { [TOKEN_HEADER]: token };
  let response;
  try {
    response = await win.fetch(path, { ...init, headers, cache: "no-store", redirect: "manual" });
  } catch {
    return { kind: "failed" };
  }
  if (response.type === "opaqueredirect" || TURNED_AWAY.has(response.status)) {
    return { kind: "away" };
  }
  if (response.status === 204) {
    return { kind: "done", body: null };
  }
  if (response.status !== 200 && !REFUSED.has(response.status)) {
    return { kind: "failed" };
  }
  let body;
  try {
    body = await response.json();
  } catch {
    return { kind: "failed" };
  }
  if (response.status === 200) {
    return { kind: "done", body };
  }
  return typeof body?.error === "string" ? { kind: "refused", reason: body.error } : { kind: "failed" };
}

/**
 * The messages of `body` after number `after`, oldest first; those already
 * shown are left out. `null` if `body` is not a list of messages.
 */
function after(body, last) {
  if (!Array.isArray(body?.messages)) {
    return null;
  }
  const fresh = [];
  for (const message of body.messages) {
    const whole = Number.isSafeInteger(message?.id) && message.id > 0
      && typeof message.mine === "boolean" && typeof message.from_operator === "boolean"
      && typeof message.who === "string" && typeof message.sent === "string"
      && typeof message.body === "string";
    if (!whole) {
      return null;
    }
    if (message.id > last) {
      fresh.push(message);
      last = message.id;
    }
  }
  return fresh;
}

/**
 * The translations of `body` after number `last`, in the order they were
 * made; those already shown are left out. `null` if `body` is not a list
 * of translations.
 */
function made(body, last) {
  if (!Array.isArray(body?.translations)) {
    return null;
  }
  const fresh = [];
  for (const translation of body.translations) {
    const whole = Number.isSafeInteger(translation?.id) && translation.id > 0
      && Number.isSafeInteger(translation.message) && translation.message > 0
      && typeof translation.body === "string";
    if (!whole) {
      return null;
    }
    if (translation.id > last) {
      fresh.push(translation);
      last = translation.id;
    }
  }
  return fresh;
}

/** The line of message number `id` in `thread`, if the page shows it. */
function lineOf(thread, id) {
  for (const line of thread.children) {
    if (line.dataset.id === String(id)) {
      return line;
    }
  }
  return null;
}

/**
 * Writes `text` under the message of `line` as its translation, marked
 * `label`, as the page shows one (`thread.html`).
 */
function translate(doc, line, label, text) {
  let words = null;
  for (const part of line.children) {
    if (part.className === "message-translation") {
      words = part.children[1];
    }
  }
  if (!words) {
    const block = doc.createElement("div");
    block.className = "message-translation";
    const mark = doc.createElement("p");
    mark.className = "message-translated muted small";
    mark.textContent = label;
    words = doc.createElement("p");
    words.className = "message-body";
    block.append(mark, words);
    line.append(block);
  }
  words.textContent = text;
}

/** A message as the page shows one (`thread.html`), its words as text. */
function item(doc, message) {
  const line = doc.createElement("li");
  line.className = ["message", message.mine && "mine", message.from_operator && "from-operator"]
    .filter(Boolean)
    .join(" ");
  line.dataset.id = String(message.id);
  const meta = doc.createElement("p");
  meta.className = "message-meta";
  const who = doc.createElement("span");
  who.className = "message-who";
  who.textContent = message.who;
  const sent = doc.createElement("span");
  sent.className = "muted small";
  sent.textContent = message.sent;
  meta.append(who, " ", sent);
  const body = doc.createElement("p");
  body.className = "message-body";
  body.textContent = message.body;
  line.append(meta, body);
  return line;
}

/** Keeps the conversation `root` current, and sends its form's messages. */
function converse(win, root) {
  const doc = win.document;
  const kind = word(root, "chat");
  if (kind !== "box" && kind !== "page") {
    throw new Error(`a conversation is a box or a page, not ${JSON.stringify(kind)}`);
  }
  const box = kind === "box" ? root.closest("details") : null;
  if (kind === "box" && !box) {
    throw new Error("the box is not in a details element");
  }
  const thread = need(root, "[data-chat-thread]");
  const empty = need(root, "[data-chat-empty]");
  const status = need(root, "[data-chat-status]");
  const form = need(root, "form");
  const field = need(form, "textarea");
  const button = need(form, "button");
  const token = tokenOf(form);
  const summary = box && need(box, "summary");
  const messages = word(root, "messages");
  const unread = box && word(root, "unread");
  // Only the operator's page shows translations: it says after which.
  const translating = typeof root.dataset.translated === "string";
  const say = {
    expired: word(root, "expired"),
    offline: word(root, "offline"),
    unsent: word(root, "unsent"),
    unread: box && word(root, "unreadText"),
    translation: translating && word(root, "translation"),
  };
  const state = {
    /** The number of the last message shown. */
    last: box ? 0 : number(word(root, "after"), "message"),
    /** The number of the last translation shown, where any are. */
    translated: translating ? number(word(root, "translated"), "translation") : null,
    timer: null,
    /** A request for messages or the badge is out. */
    busy: false,
    /** Another was asked for while it was. */
    again: false,
    sending: false,
    /** Requests in a row that could not reach the site. */
    failures: 0,
    /**
     * What the page says of the last message sent while it holds: why the
     * site refused it, or that it could not be sent; `null` once one is.
     */
    note: null,
    /** The site turned the script away: it asks nothing more. */
    over: false,
  };

  const visible = () => doc.visibilityState === "visible";
  const inSight = () => visible() && (box === null || box.open);

  function tell(text) {
    // In the box the line takes its height from the conversation: one
    // that showed its last message shows it still, and one scrolled back
    // is left where the reader is. On a page the conversation does not
    // scroll, and nothing moves.
    const ended = thread.scrollHeight - thread.scrollTop <= thread.clientHeight + 1;
    status.textContent = text;
    status.hidden = false;
    if (ended) {
      thread.scrollTop = thread.scrollHeight;
    }
  }

  function quiet() {
    status.textContent = "";
    status.hidden = true;
  }

  /** What the page says when the site answers. */
  function settle() {
    if (state.note === null) {
      quiet();
    } else {
      tell(state.note);
    }
  }

  function reached() {
    if (state.failures > 0) {
      state.failures = 0;
      settle();
    }
  }

  function unreachable() {
    state.failures += 1;
    tell(say.offline);
  }

  function away() {
    state.over = true;
    stop();
    tell(say.expired);
  }

  function badge(on) {
    const shown = summary.querySelector(".dock-unread");
    if (on && !shown) {
      const mark = doc.createElement("span");
      mark.className = "dock-unread";
      mark.textContent = say.unread;
      summary.append(mark);
    } else if (!on && shown) {
      shown.remove();
    }
  }

  function stop() {
    win.clearTimeout(state.timer);
    state.timer = null;
  }

  function schedule() {
    stop();
    if (state.over || !visible()) {
      return;
    }
    // After failures, the usual wait doubled for each.
    const usual = inSight() ? NEAR : FAR;
    state.timer = win.setTimeout(tick, Math.min(LONGEST, usual * 2 ** state.failures));
  }

  async function pull() {
    const also = translating ? `&translated=${state.translated}` : "";
    const got = await ask(win, `${messages}?after=${state.last}${also}`, token, {});
    const fresh = got.kind === "done" ? after(got.body, state.last) : null;
    const translations = fresh && translating ? made(got.body, state.translated) : [];
    if (fresh && translations) {
      for (const message of fresh) {
        thread.append(item(doc, message));
        state.last = message.id;
      }
      for (const translation of translations) {
        // The site sends a translation with its message or after it
        // (`conversation_translated`), so the page shows the message;
        // one of a message it does not show has no place on it.
        const line = lineOf(thread, translation.message);
        if (line) {
          translate(doc, line, say.translation, translation.body);
        }
        state.translated = translation.id;
      }
      const any = thread.children.length > 0;
      thread.hidden = !any;
      empty.hidden = any;
      if (box) {
        // Read now: the site counts every reply as read.
        badge(false);
        thread.scrollTop = thread.scrollHeight;
      }
      reached();
    } else if (got.kind === "away") {
      away();
    } else {
      unreachable();
    }
  }

  async function peek() {
    const got = await ask(win, unread, token, {});
    if (got.kind === "done" && typeof got.body?.unread === "boolean") {
      badge(got.body.unread);
      reached();
    } else if (got.kind === "away") {
      away();
    } else {
      unreachable();
    }
  }

  async function tick() {
    // Nothing is asked while the page is hidden; showing it asks again.
    if (state.over || !visible()) {
      return;
    }
    if (state.busy) {
      state.again = true;
      return;
    }
    state.busy = true;
    try {
      await (inSight() ? pull() : peek());
    } finally {
      state.busy = false;
    }
    if (state.again) {
      state.again = false;
      tick();
    } else {
      schedule();
    }
  }

  async function send() {
    if (state.sending) {
      return;
    }
    state.sending = true;
    button.disabled = true;
    const fields = new win.URLSearchParams();
    for (const element of form.elements) {
      if (element.name) {
        fields.append(element.name, element.value);
      }
    }
    try {
      const got = await ask(win, messages, token, { method: "POST", body: fields });
      if (got.kind === "done") {
        field.value = "";
        state.note = null;
        quiet();
        tick();
      } else if (got.kind === "refused") {
        state.note = got.reason;
        tell(got.reason);
      } else if (got.kind === "away") {
        away();
      } else {
        state.note = say.unsent;
        tell(say.unsent);
      }
    } finally {
      state.sending = false;
      button.disabled = false;
    }
  }

  form.addEventListener("submit", (event) => {
    // Turned away, the form is the browser's again: it posts the page's
    // way, and the site answers as it answers any page.
    if (!state.over) {
      event.preventDefault();
      send();
    }
  });
  doc.addEventListener("visibilitychange", () => {
    if (visible()) {
      tick();
    } else {
      stop();
    }
  });
  if (box) {
    box.addEventListener("toggle", () => {
      if (box.open) {
        tick();
      } else {
        schedule();
      }
    });
  }
  schedule();
}

if (typeof window !== "undefined") {
  start(window);
}
