// Lily Computer — Chrome extension service worker.
//
// Bridges a local lilyd daemon over WebSocket to chrome.* APIs.
// Inspired by Browser Use's "selector map" pattern: every interactive
// element on a page gets a small integer hint id (stored on window)
// so the LLM can click by id instead of guessing a CSS selector.

const DAEMON_URL = 'ws://127.0.0.1:7777/ws/chrome';
let ws = null;
let reconnectDelay = 1000;

function log(...a) { console.log('[lily]', ...a); }

function connect() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;
  log('connecting →', DAEMON_URL);
  try { ws = new WebSocket(DAEMON_URL); } catch (e) { log('ws ctor failed', e); scheduleReconnect(); return; }
  ws.onopen = () => { log('connected'); reconnectDelay = 1000; };
  ws.onclose = (e) => { log('closed', e.code, e.reason); scheduleReconnect(); };
  ws.onerror = (e) => log('error', e?.message || e);
  ws.onmessage = async (ev) => {
    let msg; try { msg = JSON.parse(ev.data); } catch { return; }
    const id = msg.id;
    try {
      const result = await dispatch(msg.cmd, msg.args || {});
      send({ id, ok: result.ok !== false, ...result });
    } catch (e) {
      send({ id, ok: false, summary: String(e?.message || e) });
    }
  };
}

function scheduleReconnect() {
  setTimeout(connect, reconnectDelay);
  reconnectDelay = Math.min(reconnectDelay * 2, 15000);
}

function send(obj) {
  if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(obj));
}

async function getActiveTab() {
  let [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab) [tab] = await chrome.tabs.query({ active: true });
  if (!tab) throw new Error('no active tab');
  if (tab.url && (tab.url.startsWith('chrome://') || tab.url.startsWith('chrome-extension://') || tab.url.startsWith('chrome.google.com/webstore'))) {
    throw new Error(`active tab is a restricted URL (${tab.url}) — Chrome will not let extensions script it. Navigate to an http(s):// page first.`);
  }
  return tab;
}

async function execInPage(tabId, func, args = [], opts = {}) {
  const r = await chrome.scripting.executeScript({
    target: { tabId, allFrames: !!opts.allFrames },
    func, args,
    world: opts.world || 'MAIN',
  });
  // Returns per-frame results when allFrames is true.
  return opts.allFrames ? r : r[0]?.result;
}

// Default hint cap for auto-attached responses — small enough that token
// budget stays sane, big enough that the model can usually see what it needs.
const AUTO_HINT_LIMIT = 60;

async function autoState(tab, opts = {}) {
  // Always-on context the LLM needs to plan the next action.
  try {
    const fresh = await chrome.tabs.get(tab.id);
    const limit = opts.maxHints ?? AUTO_HINT_LIMIT;
    let hints = [];
    try {
      hints = await execInPage(tab.id, collectHints, [limit]);
    } catch (e) {
      hints = [];
    }
    return {
      url: fresh.url,
      title: fresh.title,
      hints,
    };
  } catch {
    return {};
  }
}

// Some sites swap DOM asynchronously after a click (SPAs, autocompletes).
// A short settle helps hints reflect post-click state.
async function settle(ms = 250) {
  return new Promise(r => setTimeout(r, ms));
}

async function dispatch(cmd, args) {
  switch (cmd) {

    case 'screenshot': {
      const tab = await getActiveTab();
      const dataUrl = await chrome.tabs.captureVisibleTab(tab.windowId, { format: 'png' });
      return { summary: `captured ${tab.title || tab.url}`, image: dataUrl, tab_id: tab.id, url: tab.url };
    }

    case 'navigate': {
      const { url, new_tab } = args;
      if (!url) throw new Error('navigate: url required');
      const t = new_tab
        ? await chrome.tabs.create({ url, active: true })
        : await getActiveTab().then(t => chrome.tabs.update(t.id, { url }));
      await waitForLoad(t.id, 10000);
      await settle(150);
      const state = await autoState(t);
      return { summary: state.url || url, ...state };
    }

    case 'back':    { const t = await getActiveTab(); await chrome.tabs.goBack(t.id);    await waitForLoad(t.id, 5000); await settle(150); const s = await autoState(t); return { summary: `back → ${s.url}`, ...s }; }
    case 'forward': { const t = await getActiveTab(); await chrome.tabs.goForward(t.id); await waitForLoad(t.id, 5000); await settle(150); const s = await autoState(t); return { summary: `forward → ${s.url}`, ...s }; }
    case 'reload':  { const t = await getActiveTab(); await chrome.tabs.reload(t.id);    await waitForLoad(t.id, 8000); await settle(150); const s = await autoState(t); return { summary: 'reloaded', ...s }; }

    case 'hints': {
      const tab = await getActiveTab();
      const r = await execInPage(tab.id, collectHints, [Math.min(args.max || 200, 500)]);
      return { summary: `${r.length} hints`, hints: r, url: tab.url, title: tab.title };
    }

    case 'click': {
      const tab = await getActiveTab();
      const target = args.selector ?? args.target ?? '';
      const r = await execInPage(tab.id, clickInPage, [target]);
      if (!r?.found) {
        const state = await autoState(tab);
        return { ok: false, summary: `not found: ${target}`, tried: r?.tried, ...state };
      }
      // Wait for any navigation or DOM mutation kicked off by the click.
      await waitForLoad(tab.id, 4000);
      await settle(300);
      const state = await autoState(tab);
      return { summary: `clicked ${target}${r.text ? ' — ' + r.text.slice(0, 60) : ''}`, ...state };
    }

    case 'type': {
      const tab = await getActiveTab();
      const target = args.selector ?? args.target ?? '';
      const r = await execInPage(tab.id, typeInPage, [target, args.text || '', !!args.submit]);
      if (!r?.found) {
        const state = await autoState(tab);
        return { ok: false, summary: `not found: ${target}`, ...state };
      }
      if (args.submit) {
        await waitForLoad(tab.id, 4000);
        await settle(300);
      } else {
        await settle(150);
      }
      const state = await autoState(tab);
      return { summary: `typed ${(args.text || '').length} chars into ${target}${args.submit ? ' + Enter' : ''}`, ...state };
    }

    case 'read_page': {
      const tab = await getActiveTab();
      const max = Math.max(500, Math.min(40000, args.max_chars || 16000));
      const r = await execInPage(tab.id, readPage, [max]);
      // Don't auto-attach hints here — page text already carries the content,
      // and the response would balloon.
      return { summary: `${r.title} · ${r.text.length} chars`, ...r };
    }

    case 'query': {
      const tab = await getActiveTab();
      const r = await execInPage(tab.id, queryPage, [args.selector, Math.min(args.limit || 20, 100)]);
      return { summary: `${r.length} matches for ${args.selector}`, matches: r };
    }

    case 'wait_for': {
      const tab = await getActiveTab();
      const r = await execInPage(tab.id, waitForSel, [args.selector, Math.min(args.timeout_ms || 5000, 30000)]);
      if (!r?.found) {
        const state = await autoState(tab);
        return { ok: false, summary: `timed out waiting for ${args.selector}`, ...state };
      }
      await settle(150);
      const state = await autoState(tab);
      return { summary: `found ${args.selector}${r.text ? ' — ' + r.text.slice(0, 60) : ''}`, ...state };
    }

    case 'tabs': {
      const tabs = await chrome.tabs.query({});
      return {
        summary: `${tabs.length} tabs`,
        tabs: tabs.map(t => ({ id: t.id, title: t.title, url: t.url, active: t.active, window_id: t.windowId })),
      };
    }

    case 'switch_tab': {
      const id = Number(args.id);
      const t = await chrome.tabs.update(id, { active: true });
      await settle(200);
      const state = await autoState(t);
      return { summary: `switched to tab ${id}`, ...state };
    }

    case 'scroll': {
      const tab = await getActiveTab();
      await execInPage(tab.id, scrollPage, [args.dy ?? 600, args.to || null]);
      await settle(200);
      const state = await autoState(tab);
      return { summary: args.to ? `scrolled to ${args.to}` : `scrolled ${args.dy ?? 600}px`, ...state };
    }

    default:
      throw new Error(`unknown cmd: ${cmd}`);
  }
}

function waitForLoad(tabId, timeoutMs) {
  return new Promise((resolve) => {
    let done = false;
    const finish = () => { if (!done) { done = true; chrome.tabs.onUpdated.removeListener(listener); resolve(); } };
    const listener = (id, change) => { if (id === tabId && change.status === 'complete') finish(); };
    chrome.tabs.onUpdated.addListener(listener);
    setTimeout(finish, timeoutMs);
  });
}

// ─── functions injected into the page (MAIN world) ───────────────────────────

function collectHints(maxHints) {
  // Browser-use-style "selector map" — assign small integer ids to every visible
  // interactive element so the LLM can target by id instead of guessing CSS.
  const SELECTOR = [
    'a[href]', 'button', 'input:not([type="hidden"])', 'select', 'textarea',
    '[role="button"]', '[role="link"]', '[role="menuitem"]', '[role="tab"]',
    '[role="checkbox"]', '[role="radio"]', '[role="option"]', '[role="combobox"]',
    '[role="textbox"]', '[onclick]', '[contenteditable=""]', '[contenteditable="true"]',
    '[tabindex]:not([tabindex="-1"])',
  ].join(',');

  const els = Array.from(document.querySelectorAll(SELECTOR));
  window.__lily_hint_map = {};
  const out = [];
  let id = 1;

  for (const el of els) {
    if (out.length >= maxHints) break;
    const rect = el.getBoundingClientRect();
    if (rect.width < 4 || rect.height < 4) continue;
    const style = getComputedStyle(el);
    if (style.visibility === 'hidden' || style.display === 'none' || +style.opacity === 0) continue;

    const onScreen = rect.bottom > 0 && rect.top < window.innerHeight + 800;
    if (!onScreen) continue;

    // Skip elements that fully contain another interactive el (avoid double-listing wrappers).
    const inner = el.querySelector(SELECTOR);
    if (inner && el !== inner && el.contains(inner) && el.tagName !== 'A' && el.tagName !== 'BUTTON') continue;

    const label = (
      el.getAttribute('aria-label') ||
      el.innerText ||
      el.value ||
      el.placeholder ||
      el.title ||
      el.alt ||
      ''
    ).trim().replace(/\s+/g, ' ').slice(0, 120);

    const role = el.getAttribute('role') || (
      el.tagName === 'A' ? 'link' :
      el.tagName === 'BUTTON' ? 'button' :
      el.tagName === 'INPUT' ? (el.type || 'input') :
      el.tagName.toLowerCase()
    );

    window.__lily_hint_map[id] = el;
    out.push({
      id,
      role,
      text: label,
      tag: el.tagName.toLowerCase(),
      href: el.href || null,
    });
    id++;
  }
  return out;
}

function clickInPage(target) {
  let el = null;
  const tried = [];

  // hint:N — look up the cached selector map.
  if (typeof target === 'string' && target.startsWith('hint:')) {
    tried.push('hint');
    const n = Number(target.slice(5));
    el = (window.__lily_hint_map || {})[n];
    if (!el) return { found: false, tried, reason: 'unknown hint id — call browser_hints again' };
  }
  // text:substring — case-insensitive substring on innerText/aria-label.
  else if (typeof target === 'string' && target.startsWith('text:')) {
    tried.push('text');
    const needle = target.slice(5).trim().toLowerCase();
    const candidates = document.querySelectorAll('a, button, [role="button"], [role="link"], [role="menuitem"], [role="tab"], [aria-label]');
    for (const c of candidates) {
      const t = ((c.getAttribute('aria-label') || c.innerText || '').trim()).toLowerCase();
      if (t && t.includes(needle)) { el = c; break; }
    }
  }
  // raw CSS selector
  else {
    tried.push('css');
    try { el = document.querySelector(target); } catch {}
  }

  if (!el) return { found: false, tried };
  el.scrollIntoView({ block: 'center', behavior: 'instant' });
  // Some sites attach handlers on mousedown — synthesize a full sequence.
  el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  el.click();
  return {
    found: true,
    text: (el.getAttribute('aria-label') || el.innerText || el.value || '').slice(0, 100),
  };
}

function typeInPage(target, text, submit) {
  let el = null;
  if (target.startsWith('hint:')) {
    el = (window.__lily_hint_map || {})[Number(target.slice(5))];
  } else {
    try { el = document.querySelector(target); } catch {}
  }
  if (!el) return { found: false };
  el.focus();
  if ('value' in el && (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
    setter ? setter.call(el, text) : (el.value = text);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  } else {
    el.textContent = text;
    el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text }));
  }
  if (submit) {
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true }));
    el.dispatchEvent(new KeyboardEvent('keyup',   { key: 'Enter', code: 'Enter', bubbles: true }));
    if (el.form && typeof el.form.requestSubmit === 'function') el.form.requestSubmit();
  }
  return { found: true };
}

function readPage(maxChars) {
  const text = (document.body && document.body.innerText) || '';
  return {
    url: location.href,
    title: document.title,
    text: text.slice(0, maxChars),
    truncated: text.length > maxChars,
    full_length: text.length,
  };
}

function queryPage(selector, limit) {
  let els = [];
  try { els = [...document.querySelectorAll(selector)].slice(0, limit); } catch { return []; }
  return els.map((el, i) => ({
    index: i,
    tag: el.tagName.toLowerCase(),
    text: (el.innerText || el.value || '').slice(0, 200),
    href: el.href || null,
    aria_label: el.getAttribute && el.getAttribute('aria-label'),
    id: el.id || null,
    classes: typeof el.className === 'string' ? el.className.slice(0, 100) : null,
  }));
}

async function waitForSel(selector, timeoutMs) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    let el = null;
    try { el = document.querySelector(selector); } catch {}
    if (el) {
      const r = el.getBoundingClientRect();
      if (r.width > 0 && r.height > 0) {
        return { found: true, text: (el.innerText || el.value || '').slice(0, 100) };
      }
    }
    await new Promise(r => setTimeout(r, 120));
  }
  return { found: false };
}

function scrollPage(dy, to) {
  if (to === 'top') window.scrollTo({ top: 0, behavior: 'instant' });
  else if (to === 'bottom') window.scrollTo({ top: document.body.scrollHeight, behavior: 'instant' });
  else window.scrollBy({ top: dy, behavior: 'instant' });
}

// ─── entry ──────────────────────────────────────────────────────────────────

connect();

chrome.alarms?.create?.('lily-heartbeat', { periodInMinutes: 0.5 });
chrome.alarms?.onAlarm?.addListener?.(() => connect());

chrome.runtime.onMessage.addListener((msg, _sender, respond) => {
  if (msg?.type === 'reconnect') {
    try { ws?.close(); } catch {}
    ws = null;
    reconnectDelay = 1000;
    connect();
    respond({ ok: true });
    return true;
  }
});
