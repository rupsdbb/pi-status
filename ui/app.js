// SPDX-License-Identifier: GPL-3.0-or-later
"use strict";

/* ============================== helpers ============================== */

const $ = (sel, el = document) => el.querySelector(sel);
const ESC = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ESC[c]);
const clamp01 = (x) => Math.min(1, Math.max(0, x || 0));
const fin = (x) => x != null && isFinite(x);

const store = {
  get(k, d) { try { return localStorage.getItem(k) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* storage unavailable */ } },
};

/** Decimal units, e.g. [7.4, "GB"]. */
function bytesParts(n) {
  if (!fin(n)) return ["--", ""];
  const u = ["B", "KB", "MB", "GB", "TB", "PB"];
  let i = 0;
  while (n >= 1000 && i < u.length - 1) { n /= 1000; i++; }
  let text = i === 0 ? String(Math.round(n)) : n.toFixed(n < 10 ? 1 : 0);
  if (Number(text) >= 1000 && i < u.length - 1) { n /= 1000; i++; text = n.toFixed(1); } // 999.95 KB → 1.0 MB
  return [text, u[i]];
}
const bytes = (n) => bytesParts(n).join(" ").trim();
const rate = (n) => (fin(n) ? `${bytes(n)}/s` : "--");
const num = (n) => (fin(n) ? Number(n).toLocaleString("en-US") : "--");
const pct = (x, dp = 0) => (fin(x) ? `${x.toFixed(dp)}%` : "--");

function dur(s) {
  if (!fin(s)) return "--";
  s = Math.max(0, Math.floor(s));
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m`;
  return `${s}s`;
}
const serverNow = () => Date.now() / 1000 + S.offset;
const ago = (ts) => (!ts ? "--" : serverNow() - ts < 5 ? "just now" : `${dur(serverNow() - ts)} ago`);
const dateShort = (ts) =>
  new Date(ts * 1000).toLocaleString("en-GB", { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" });

function level(v, warn, crit) {
  if (!fin(v)) return "";
  if (fin(crit) && v >= crit) return "crit";
  if (fin(warn) && v >= warn) return "warn";
  return "ok";
}

/* ============================ dot graphics ============================ */
/* Every dot graphic is two <path>s (lit and unlit dots) rather than one
   element per dot: ~1,500 fewer DOM nodes rebuilt on each refresh. */

const G = 8;      // dot pitch
const R = 2.6;    // dot radius

const dotAt = (x, y, r = R) =>
  `M${(x - r).toFixed(2)},${y.toFixed(2)}a${r},${r} 0 1,0 ${2 * r},0a${r},${r} 0 1,0 ${-2 * r},0`;

function dotsSvg(w, h, on, off) {
  return `<svg class="dots" viewBox="0 0 ${w} ${h}" aria-hidden="true">` +
    (off ? `<path class="off" d="${off}"/>` : "") + (on ? `<path class="on" d="${on}"/>` : "") + `</svg>`;
}

/** History as a dot-matrix column chart, newest on the right. */
function dotChart(values, { cols = 24, rows = 7, min = null, max = null } = {}) {
  values = values || [];
  const per = Math.max(1, Math.floor(values.length / cols));
  const buckets = [];
  for (let end = values.length; end > 0 && buckets.length < cols; end -= per) {
    const sl = values.slice(Math.max(0, end - per), end).filter(fin);
    buckets.unshift(sl.length ? sl.reduce((a, b) => a + b, 0) / sl.length : null);
  }
  while (buckets.length < cols) buckets.unshift(null);

  const vs = buckets.filter(fin);
  let lo = min ?? (vs.length ? Math.min(...vs) : 0);
  let hi = max ?? (vs.length ? Math.max(...vs) : 1);
  if (min == null && max == null) {
    const pad = (hi - lo) * 0.35 || Math.max(Math.abs(hi) * 0.05, 0.5);
    lo -= pad; hi += pad;
  } else if (max == null) {
    hi = Math.max(hi * 1.2, lo + 1e-9);
  }

  let on = "", off = "";
  buckets.forEach((v, c) => {
    const lit = !fin(v) ? 0 : Math.min(rows, Math.max(1, Math.round(((v - lo) / (hi - lo || 1)) * rows)));
    for (let r = 0; r < rows; r++) {
      const d = dotAt(c * G + G / 2, r * G + G / 2);
      if (rows - r <= lit) on += d; else off += d;
    }
  });
  return dotsSvg(cols * G, rows * G, on, off);
}

/** Single row of dots, lit left to right. */
function dotMeter(frac, n = 32) {
  const lit = Math.round(clamp01(frac) * n);
  let on = "", off = "";
  for (let i = 0; i < n; i++) {
    const d = dotAt(i * G + G / 2, G / 2);
    if (i < lit) on += d; else off += d;
  }
  return dotsSvg(n * G, G, on, off);
}

/** Ring of dots, lit clockwise from 12 o'clock. */
function dotRing(frac, n = 44) {
  const lit = Math.round(clamp01(frac) * n);
  let on = "", off = "";
  for (let i = 0; i < n; i++) {
    const a = (i / n) * 2 * Math.PI - Math.PI / 2;
    const d = dotAt(50 + 45 * Math.cos(a), 50 + 45 * Math.sin(a), 2.7);
    if (i < lit) on += d; else off += d;
  }
  return dotsSvg(100, 100, on, off);
}

/** Vertical column of dots, lit bottom-up. */
function dotColumn(frac, n = 6) {
  const lit = Math.round(clamp01(frac) * n);
  let on = "", off = "";
  for (let i = 0; i < n; i++) {
    const d = dotAt(G / 2, i * G + G / 2);
    if (n - i <= lit) on += d; else off += d;
  }
  return dotsSvg(G, n * G, on, off);
}

/* =============================== state =============================== */

const S = {
  data: null,
  offset: 0,
  lastOk: 0,
  error: null,
  timer: null,
  filter: store.get("pi-status.filter", "all"),
  alertsOpen: false,
  sheet: null,          // { kind: "svc" | "log", id }
  log: { lines: [], updated: null, error: null },
};

/* ============================== modules ============================== */

/** Dot-matrix headline value. Doto is monospaced (0.61em per character), so the
    CSS can shrink long values (e.g. a 400-day uptime) to fit the card exactly. */
function bigNum(value, unit = "") {
  const text = String(value);
  const unitW = unit ? unit.length * 9 + 6 : 0;   // Space Mono unit + gap, in px
  return `<div class="big" style="--chars:${text.length};--unit-w:${unitW}px">${esc(text)}` +
    (unit ? `<span class="unit">${esc(unit)}</span>` : "") + `</div>`;
}


const head = (l, r = "") =>
  `<header class="mod-head"><span class="label">${l}</span>${r ? `<span class="label">${r}</span>` : ""}</header>`;

function windowLabel(d, series) {
  return dur((series?.length || 0) * d.interval);
}

function vitals(d) {
  const s = d.system, t = d.thresholds, h = d.history;
  if (!s) return [];
  const mods = [];
  const win = windowLabel(d, h.cpu);

  const cpuAvg = h.cpu.filter(fin);
  mods.push({
    id: "cpu", cls: "m-sm", s: level(s.cpu_pct, t.cpu_warn),
    html: head("CPU", `${win}`) +
      bigNum(fin(s.cpu_pct) ? Math.round(s.cpu_pct) : "--", "%") +
      `<div class="note">avg ${cpuAvg.length ? pct(cpuAvg.reduce((a, b) => a + b, 0) / cpuAvg.length) : "--"}</div>` +
      `<div class="chart" data-s="${level(s.cpu_pct, t.cpu_warn)}">${dotChart(h.cpu, { min: 0, max: 100 })}</div>`,
  });

  const temps = h.temp.filter(fin);
  mods.push({
    id: "temp", cls: "m-sm", s: level(s.temp_c, t.temp_warn, t.temp_crit),
    html: head("Temp", win) +
      bigNum(fin(s.temp_c) ? s.temp_c.toFixed(1) : "--", "°C") +
      `<div class="note">peak ${temps.length ? Math.max(...temps).toFixed(1) + "°" : "--"}</div>` +
      `<div class="chart" data-s="${level(s.temp_c, t.temp_warn, t.temp_crit)}">${dotChart(h.temp)}</div>`,
  });

  const memPct = s.mem.total ? (s.mem.used * 100) / s.mem.total : null;
  mods.push({
    id: "mem", cls: "m-sm", s: level(memPct, t.mem_warn),
    html: head("Memory", bytes(s.mem.total)) +
      bigNum(fin(memPct) ? Math.round(memPct) : "--", "%") +
      `<div class="note">${bytes(s.mem.used)} used</div>` +
      `<div class="chart" data-s="${level(memPct, t.mem_warn)}">${dotChart(h.mem, { min: 0, max: 100 })}</div>`,
  });

  const [rxN, rxU] = bytesParts(s.net?.rx_bps);
  mods.push({
    id: "net", cls: "m-sm", s: "",
    html: head("Net in", esc(s.net?.iface ?? "")) +
      bigNum(rxN, rxU ? rxU + "/s" : "") +
      `<div class="note">out ${rate(s.net?.tx_bps)}</div>` +
      `<div class="chart">${dotChart(h.rx, { min: 0 })}</div>`,
  });

  mods.push({
    id: "load", cls: "m-sm", s: s.load[0] > s.cpu_count ? "warn" : "",
    html: head("Load", `${s.cpu_count} cores`) +
      bigNum(s.load[0].toFixed(2)) +
      `<div class="note">${s.load[1].toFixed(2)} · ${s.load[2].toFixed(2)}</div>` +
      `<div class="chart" data-s="${s.load[0] > s.cpu_count ? "warn" : ""}">${dotChart(h.load, { min: 0 })}</div>`,
  });

  mods.push({
    id: "uptime", cls: "m-sm", s: "",
    html: head("Uptime") +
      bigNum(dur(s.uptime_secs).toUpperCase()) +
      `<div class="note">since ${dateShort(s.boot_time)}</div>` +
      `<dl class="spec mini"><dt>Dashboard</dt><dd>up ${dur(serverNow() - d.started)}</dd>` +
      `<dt>Interval</dt><dd>${d.interval}s</dd></dl>`,
  });

  return mods;
}

function bitcoinMod(d) {
  const b = d.bitcoin;
  if (!b) return null;
  if (b.error) {
    return { id: "bitcoin", cls: "m-sm", s: "warn", html: head("Bitcoin") + `<div class="clip flush"><div class="err">${esc(b.error)}</div></div>` };
  }
  const chain = { main: "mainnet", test: "testnet3", testnet4: "testnet4", signet: "signet", regtest: "regtest" }[b.chain] ?? b.chain;
  const behind = Math.max(0, b.headers - b.blocks);
  const synced = !b.ibd && behind <= 1;
  const mpFrac = b.mempool_max ? b.mempool_usage / b.mempool_max : 0;
  const ver = /Satoshi:([\d.]+)/.exec(b.subversion)?.[1];
  const node = ver ? `${/knots/i.test(b.subversion) ? "Knots" : "Core"} ${ver}` : b.subversion || "--";

  return {
    id: "bitcoin", cls: "m-wide", s: b.peers === 0 ? "warn" : "",
    html: head("Bitcoin", esc(chain)) +
      `<div class="split btc"><div>` +
      bigNum(num(b.blocks)) +
      `<div class="note">${synced ? `synced · block ${ago(b.best_block_time)}` : `syncing · ${num(behind)} behind`}</div>` +
      `<div class="meter push">${dotMeter(synced ? mpFrac : b.progress, 30)}</div>` +
      `<div class="meter-cap"><span class="label">${synced ? "Mempool fill" : "Sync"}</span>` +
      `<span class="label">${synced ? pct(mpFrac * 100) : pct(b.progress * 100, 2)}</span></div>` +
      `</div><dl class="spec">` +
      `<dt>Peers</dt><dd>${b.peers} <small>${b.peers_in}↓ ${b.peers_out}↑</small></dd>` +
      `<dt>Mempool</dt><dd>${num(b.mempool_tx)} <small>tx</small></dd>` +
      `<dt>Min fee</dt><dd>${b.mempool_min_fee.toFixed(2)} <small>sat/vB</small></dd>` +
      `<dt>Chain</dt><dd>${bytes(b.size_on_disk)}${b.pruned ? " <small>pruned</small>" : ""}</dd>` +
      `<dt>Node</dt><dd title="${esc(b.subversion)}">${esc(node)}</dd>` +
      `</dl></div>`,
  };
}

function flagsHtml(th) {
  if (!th || th.error) return "";
  const f = (name, now, seen) =>
    `<span class="flag" data-s="${now ? "crit" : seen ? "warn" : ""}"><span class="led" data-s="${now ? "crit" : seen ? "warn" : "ok"}"></span>${name}</span>`;
  return `<div class="flags">` +
    f("UV", th.undervoltage_now, th.undervoltage_seen) +
    f("THR", th.throttled_now, th.throttled_seen) +
    f("CAP", th.freq_capped_now, th.freq_capped_seen) +
    f("TMP", th.soft_temp_limit_now, th.soft_temp_limit_seen) +
    `</div>`;
}

function powerMod(d) {
  const u = d.ups, th = d.system?.throttled, t = d.thresholds;
  if (!u && !th) return null;
  const thLevel = !th || th.error ? "" : th.undervoltage_now ? "crit" : th.undervoltage_seen || th.throttled_now ? "warn" : "";

  if (!u || u.error) {
    const body = (u?.error ? `<div class="err">${esc(u.error)}</div>` : "") +
      (th?.error ? `<div class="err">${esc(th.error)}</div>` : "") +
      (th && !th.error
        ? `<dl class="spec gap-top">` +
          [["Under-volt", th.undervoltage_now, th.undervoltage_seen], ["Throttled", th.throttled_now, th.throttled_seen],
           ["Freq cap", th.freq_capped_now, th.freq_capped_seen], ["Temp limit", th.soft_temp_limit_now, th.soft_temp_limit_seen]]
            .map(([k, now, seen]) => `<dt>${k}</dt><dd>${now ? "NOW" : seen ? "SINCE BOOT" : "NONE"}</dd>`).join("") +
          `</dl>`
        : "");
    return { id: "power", cls: "m-sm", s: u?.error ? "warn" : thLevel, html: head("Power") + `<div class="clip flush">${body}</div>` };
  }

  const battLevel = u.percent <= t.battery_crit ? "crit" : u.percent <= t.battery_warn ? "warn" : "";
  const s = battLevel === "crit" || thLevel === "crit" ? "crit" : battLevel || thLevel || (u.on_battery ? "warn" : "");
  const time = fin(u.minutes_to_empty) ? `${dur(u.minutes_to_empty * 60)} left`
    : fin(u.minutes_to_full) ? `full in ${dur(u.minutes_to_full * 60)}` : u.state;
  const cells = (u.cells_mv || []).map((mv) =>
    `<div class="cell" data-s="${mv < 3300 ? "crit" : mv < 3500 ? "warn" : ""}" title="${mv} mV">${dotColumn((mv - 3000) / 1200, 5)}<span>${(mv / 1000).toFixed(2)}</span></div>`).join("");

  return {
    id: "power", cls: "m-wide", s,
    html: head("Power", esc(u.on_battery ? "on battery" : u.state)) +
      `<div class="split pwr">` +
      `<div class="ring" data-s="${battLevel}">${dotRing(u.percent / 100)}` +
      `<div class="ring-txt"><b>${u.percent}%</b><span class="label">${esc(time)}</span></div></div>` +
      `<div><dl class="spec">` +
      `<dt>Input</dt><dd>${(u.input_mw / 1000).toFixed(1)} W <small>${(u.input_mv / 1000).toFixed(2)} V</small></dd>` +
      `<dt>Battery</dt><dd>${u.battery_ma > 0 ? "+" : ""}${u.battery_ma} mA <small>${(u.battery_mv / 1000).toFixed(2)} V</small></dd>` +
      `<dt>Charge</dt><dd>${num(u.remaining_mah)} mAh</dd>` +
      `</dl>` +
      (cells ? `<div class="cells">${cells}</div>` : "") +
      flagsHtml(th) +
      `</div></div>`,
  };
}

function storageMod(d) {
  if (!d.storage.length) return null;
  const t = d.thresholds;
  let worst = "";
  const rows = d.storage.map((m) => {
    if (m.error) {
      worst = worst || "warn";
      return `<div class="disk" data-s="warn"><div class="disk-head"><span class="disk-name">${esc(m.label)}<small>${esc(m.path)}</small></span>` +
        `<span class="label hot">${esc(m.error)}</span></div><div class="meter">${dotMeter(0, 40)}</div></div>`;
    }
    const p = m.used + m.avail ? (m.used * 100) / (m.used + m.avail) : 0;
    const l = level(p, t.disk_warn, t.disk_crit);
    if (l === "crit" || (l === "warn" && !worst)) worst = l;
    return `<div class="disk" data-s="${l === "ok" ? "" : l}"><div class="disk-head">` +
      `<span class="disk-name" title="${esc(m.device)}">${esc(m.label)}<small>${esc(m.path)}</small></span>` +
      `<span class="disk-pct">${Math.round(p)}%</span></div>` +
      `<div class="meter tight" data-s="${l === "ok" ? "" : l}">${dotMeter(p / 100, 40)}</div>` +
      `<div class="meter-cap"><span class="label">${bytes(m.used)} / ${bytes(m.total)}</span><span class="label">${bytes(m.avail)} free</span></div></div>`;
  }).join("");
  return { id: "storage", cls: "m-wide", s: worst, html: head("Storage", `${d.storage.length} mounts`) + `<div class="disks clip">${rows}</div>` };
}

function logsMod(d) {
  if (!d.logs.length) return null;
  let s = "";
  const rows = d.logs.map((l) => {
    const hot = !!l.error || (l.alert && l.count > 0);
    if (hot) s = "warn";
    const last = l.error ? l.error : l.last ? l.last.replace(/^\S+\s+\S+\s+/, "") : "no entries";
    return `<button type="button" class="row-btn" data-log="${esc(l.id)}">` +
      `<span class="row-name">${esc(l.name)}</span><span class="row-count${hot ? " hot" : ""}">${l.error ? "!" : l.count == null ? "--" : num(l.count)}</span>` +
      `<span class="row-last">${esc(last)}</span></button>`;
  }).join("");
  return { id: "logs", cls: "m-wide", s, html: head("Logs", "this boot") + `<div class="clip"><div class="rows">${rows}</div></div>` };
}

function panelMods(d) {
  return d.panels.map((p) => {
    const data = p.data;
    let body = "";
    if (!data) body = `<div class="note">waiting for first run</div>`;
    else {
      if (data.error) body += `<div class="err">${esc(data.error)}</div>`;
      if (data.items.length) body += `<dl class="spec">${data.items.map((i) => `<dt>${esc(i.key)}</dt><dd>${esc(i.value)}</dd>`).join("")}</dl>`;
      if (data.lines.length) body += `<pre class="out">${esc(data.lines.join("\n"))}</pre>`;
      if (!data.error && !data.items.length && !data.lines.length) body += `<div class="note">no output</div>`;
    }
    return {
      id: `panel:${p.id}`, cls: p.wide ? "m-big" : "m-wide", s: data?.error ? "warn" : "", open: `panel:${p.id}`, label: p.name,
      html: head(esc(p.name), "open") + `<div class="clip">${body}</div>` +
        (data ? `<div class="foot-note">updated ${ago(data.updated)}</div>` : ""),
    };
  });
}

/** Keep module elements stable (preserves scroll positions) and in order. */
function reconcile(root, mods) {
  const keep = new Set();
  mods.forEach((m, i) => {
    keep.add(m.id);
    let el = root.querySelector(`:scope > [data-mod="${CSS.escape(m.id)}"]`);
    if (!el) {
      el = document.createElement("article");
      el.dataset.mod = m.id;
    }
    el.className = `mod ${m.cls}`;
    el.dataset.s = m.s || "";
    if (m.open) {
      el.dataset.open = m.open;
      el.tabIndex = 0;
      el.setAttribute("role", "button");
      el.setAttribute("aria-label", `Open ${m.label ?? m.id}`);
    } else {
      delete el.dataset.open;
      el.removeAttribute("tabindex");
      el.removeAttribute("role");
      el.removeAttribute("aria-label");
    }
    if (el._html !== m.html) {
      el.innerHTML = m.html;
      el._html = m.html;
    }
    if (root.children[i] !== el) root.insertBefore(el, root.children[i] || null);
  });
  [...root.children].forEach((c) => { if (!keep.has(c.dataset.mod)) c.remove(); });
  markClipped(root);
}

/* Cards never scroll internally (that would trap wheel and swipe gestures and
   stop the page from scrolling). Content that doesn't fit fades out instead,
   and the full content opens in the sheet. */
function markClipped(root) {
  root.querySelectorAll(".clip").forEach((el) => {
    el.classList.toggle("more", el.scrollHeight > el.clientHeight + 1);
  });
}
window.addEventListener("resize", () => markClipped($("#bento")));
document.fonts?.ready.then(() => markClipped($("#bento")));   // heights change once fonts load

document.addEventListener("keydown", (e) => {
  const card = e.target.closest?.("#bento [data-open]");
  if (card && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); card.click(); }
});

function renderBento(d) {
  const mods = [...vitals(d), bitcoinMod(d), powerMod(d), storageMod(d), logsMod(d), ...panelMods(d)].filter(Boolean);
  reconcile($("#bento"), mods);
}

/* ============================== services ============================== */

function svcLevel(s) {
  if (s.load === "not-found") return "warn";
  switch (s.active) {
    case "active": return "ok";
    case "failed": return "crit";
    case "inactive": return "warn";
    case "activating": case "deactivating": case "reloading": return "info";
    default: return "off";
  }
}

/** One word for the sheet: running / exited / stopped / failed / not found / ... */
function svcWord(s) {
  if (s.load === "not-found") return "not found";
  if (s.active === "active") return s.sub || "active";
  if (s.active === "inactive") return "stopped";
  return s.active || "unknown";
}

function svcState(s) {
  if (s.load === "not-found") return "not found";
  if (s.active === "active") return fin(s.active_secs) ? dur(s.active_secs) : s.sub;
  if (s.active === "inactive") return "stopped";
  return s.active || "unknown";
}

function renderServices(d) {
  const all = d.services;
  const issues = all.filter((s) => svcLevel(s) !== "ok");
  const running = all.filter((s) => s.active === "active").length;
  $("#svc-meta").textContent = all.length ? `${running} / ${all.length} running` : "";
  document.querySelectorAll(".seg button").forEach((b) => {
    b.setAttribute("aria-pressed", String(b.dataset.filter === S.filter));
    if (b.dataset.filter === "issues") b.textContent = issues.length ? `Issues ${issues.length}` : "Issues";
  });

  const list = S.filter === "issues" ? issues : all;
  let html = "";
  if (!all.length) html = `<div class="empty">No services configured — add files to services.d/</div>`;
  else if (!list.length) html = `<div class="empty"><span class="led" data-s="ok"></span>All ${all.length} services running</div>`;
  else {
    const groups = new Map();
    list.forEach((s) => { if (!groups.has(s.group)) groups.set(s.group, []); groups.get(s.group).push(s); });
    for (const [name, items] of groups) {
      const up = items.filter((s) => s.active === "active").length;
      html += `<div class="grp"><span class="label">${esc(name)}</span><span class="label">${up}/${items.length}</span></div>`;
      html += items.map((s) => {
        const l = svcLevel(s);
        return `<button type="button" class="svc" data-svc="${esc(s.id)}" data-s="${l === "ok" ? "" : l}">` +
          `<span class="svc-top"><span class="led" data-s="${l}"></span><span class="svc-name">${esc(s.name)}</span></span>` +
          `<span class="svc-bot"><span>${s.version ? "v" + esc(s.version) : ""}</span><span class="state">${esc(svcState(s))}</span></span></button>`;
      }).join("");
    }
  }
  const el = $("#services");
  if (el._html !== html) { el.innerHTML = html; el._html = html; }
}

/* =============================== header =============================== */

function renderStatus() {
  const d = S.data;
  const stale = !S.lastOk || Date.now() - S.lastOk > Math.max(15, (d?.interval ?? 5) * 3) * 1000;
  let s = "off", text = "Connecting";
  if (S.error && stale) { s = "crit"; text = S.lastOk ? "Offline" : "Unreachable"; }
  else if (d) {
    const crit = d.alerts.filter((a) => a.level === "crit").length;
    const warn = d.alerts.filter((a) => a.level === "warn").length;
    if (crit) { s = "crit"; text = `${crit + warn} issue${crit + warn > 1 ? "s" : ""}`; }
    else if (warn) { s = "warn"; text = `${warn} warning${warn > 1 ? "s" : ""}`; }
    else { s = "ok"; text = "All systems OK"; }
    if (s !== "crit" && d.now - d.updated > d.interval * 2 + 2) { s = "warn"; text = "Data stalled"; }
    document.title = (crit + warn ? `(${crit + warn}) ` : "") + d.title;
  }
  $("#status").dataset.s = s;
  $("#status-text").textContent = text;

  if (d) {
    // Server sampling and browser polling both run every `interval` seconds but
    // aren't in step, so normal data is up to ~2 intervals old: that is "live".
    //   stalled: the snapshot was already old when fetched (server collector stuck)
    //   paused:  this page hasn't polled lately (background tab, sleeping laptop)
    const slack = d.interval * 2 + 2;
    const age = serverNow() - d.updated;
    const stalledAtFetch = d.now - d.updated > slack;
    const sinceFetch = (Date.now() - S.lastOk) / 1000;
    const state = S.error ? `offline · data ${dur(age)} old`
      : stalledAtFetch ? `stalled · updated ${dur(age)} ago`
      : sinceFetch > slack ? `paused · updated ${dur(age)} ago`
      : "live";
    $("#sub").textContent = `${d.hostname} · ${state}`;
  }
}

function renderClock() {
  const n = new Date();
  $("#clock").textContent = `${String(n.getHours()).padStart(2, "0")}:${String(n.getMinutes()).padStart(2, "0")}`;
}

function renderAlerts(d) {
  const el = $("#alerts");
  const list = d.alerts;
  el.hidden = !list.length;
  if (!list.length) return;
  const limit = matchMedia("(max-width: 640px)").matches ? 2 : 3;
  const shown = S.alertsOpen ? list : list.slice(0, limit);
  el.innerHTML = shown.map((a) =>
    `<button type="button" class="alert" data-src="${esc(a.source)}">` +
    `<span class="lvl ${a.level}">${a.level.toUpperCase()}</span><span class="src">${esc(a.source)}</span>` +
    `<span class="txt">${esc(a.text)}</span></button>`).join("") +
    (list.length > limit ? `<button type="button" class="pill more" data-act="alerts">${S.alertsOpen ? "Show less" : `+${list.length - limit} more`}</button>` : "");
}

/* =============================== sheets =============================== */

const sheet = $("#sheet");

function openSheet(kind, id) {
  S.sheet = { kind, id };
  sheet.classList.toggle("compact", kind === "svc");
  $("#sheet-body").scrollTop = 0;
  $("#sheet-body")._html = null;
  $("#sheet-tools").hidden = kind !== "log";
  if (kind === "log") {
    S.log = { lines: [], updated: null, error: null };
    $("#log-filter").value = "";
    $("#sheet-body").innerHTML = `<div class="log"><div class="note">loading…</div></div>`;
    loadLog();
  }
  renderSheet();
  if (!sheet.open) sheet.showModal();
}

/** Replace the sheet body only when it changed, so live refreshes don't reset
    the reader's text selection. */
function setSheetBody(html) {
  const body = $("#sheet-body");
  if (body._html !== html) { body.innerHTML = html; body._html = html; }
}

function renderSheet(d = S.data) {
  if (!S.sheet || !d) return;
  if (S.sheet.kind === "panel") {
    const p = d.panels.find((x) => x.id === S.sheet.id);
    if (!p) { sheet.close(); return; }
    const data = p.data;
    $("#sheet-title").textContent = p.name;
    $("#sheet-meta").textContent = data ? `updated ${ago(data.updated)}` : "waiting for first run";
    let body = "";
    if (data?.error) body += `<div class="err">${esc(data.error)}</div>`;
    if (data?.items.length) body += `<dl class="spec">${data.items.map((i) => `<dt>${esc(i.key)}</dt><dd class="wrap">${esc(i.value)}</dd>`).join("")}</dl>`;
    if (data?.lines.length) body += `<pre class="out sheet-out">${esc(data.lines.join("\n"))}</pre>`;
    if (data && !data.error && !data.items.length && !data.lines.length) body += `<div class="empty">No output</div>`;
    setSheetBody(body);
    return;
  }
  if (S.sheet.kind === "svc") {
    const s = d.services.find((x) => x.id === S.sheet.id);
    if (!s) { sheet.close(); return; }
    const l = svcLevel(s);
    $("#sheet-title").textContent = s.name;
    $("#sheet-meta").textContent = s.unit;
    const row = (k, v, cls = "") => (v == null || v === "" ? "" : `<dt>${k}</dt><dd class="${cls}">${v}</dd>`);
    setSheetBody(`<dl class="spec">` +
      row("Status", `<span class="status-cell"><span class="led" data-s="${l}"></span>${esc(svcWord(s))}</span>`) +
      row("State", esc(`${s.active} / ${s.sub}`)) +
      row("Running for", fin(s.active_secs) ? dur(s.active_secs) : null) +
      row("Since", fin(s.active_secs) ? dateShort(serverNow() - s.active_secs) : null) +
      row("Memory", fin(s.memory) ? bytes(s.memory) : null) +
      row("Restarts", fin(s.restarts) ? String(s.restarts) : null) +
      row("PID", fin(s.pid) ? String(s.pid) : null) +
      (s.version ? row("Version", esc(s.version))
        : s.version_error ? row("Version", `<span class="hot-text">unavailable · ${esc(s.version_error)}</span>`, "wrap") : "") +
      row("Group", esc(s.group)) +
      row("Description", esc(s.description), "wrap") +
      (s.link ? row("Link", `<a href="${esc(s.link)}" target="_blank" rel="noopener">${esc(s.link)}</a>`, "wrap") : "") +
      `</dl>`);
  } else {
    const meta = d.logs.find((x) => x.id === S.sheet.id);
    $("#sheet-title").textContent = meta?.name ?? S.sheet.id;
    renderLog();
  }
}

const LINE_RE = /^(\S+)\s+(\S+)\s+([^:\s][^:]*?:)\s?(.*)$/;

function fmtTs(ts) {
  const d = new Date(ts.replace(/([+-]\d\d)(\d\d)$/, "$1:$2"));
  return isNaN(d) ? ts : d.toLocaleString("en-GB", { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function mark(text, q) {
  if (!q) return esc(text);
  const lc = text.toLowerCase();
  let out = "", i = 0, j;
  while ((j = lc.indexOf(q, i)) !== -1) {
    out += esc(text.slice(i, j)) + `<mark>${esc(text.slice(j, j + q.length))}</mark>`;
    i = j + q.length;
  }
  return out + esc(text.slice(i));
}

function renderLog() {
  if (S.sheet?.kind !== "log") return;
  const q = $("#log-filter").value.trim().toLowerCase();
  const all = S.log.lines;
  const lines = q ? all.filter((l) => l.toLowerCase().includes(q)) : all;
  $("#sheet-meta").textContent = `${q ? `${num(lines.length)} of ` : ""}${num(all.length)} lines · ${S.log.updated ? `updated ${ago(S.log.updated)}` : "loading"}`;
  const wrap = $("#log-wrap").getAttribute("aria-pressed") === "true";
  let inner;
  if (S.log.error) inner = `<div class="err">${esc(S.log.error)}</div>`;
  else if (!lines.length) inner = `<div class="empty">${all.length ? "No lines match" : S.log.updated ? "No entries" : "Loading…"}</div>`;
  else inner = lines.map((l) => {
    const m = LINE_RE.exec(l);
    return m
      ? `<div class="log-line"><span class="ts">${esc(fmtTs(m[1]))}</span><span class="id">${esc(m[3])}</span>${mark(m[4], q)}</div>`
      : `<div class="log-line">${mark(l, q)}</div>`;
  }).join("");
  $("#sheet-body").innerHTML = `<div class="log${wrap ? " wrap" : ""}">${inner}</div>`;
}

async function loadLog() {
  const id = S.sheet?.id;
  try {
    const r = await fetch(`api/logs/${encodeURIComponent(id)}`, { cache: "no-store" });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const j = await r.json();
    if (S.sheet?.id !== id) return;
    S.log = { lines: j.lines, updated: j.updated, error: j.error };
  } catch (e) {
    S.log = { lines: [], updated: null, error: `Could not load log: ${e.message}` };
  }
  renderLog();
  const body = $("#sheet-body");
  body.scrollTop = body.scrollHeight;
}

/* =============================== render =============================== */

function render(d) {
  const uiKey = JSON.stringify(d.ui);
  if (uiKey !== S.uiKey) {           // config defaults or custom palettes changed
    S.uiKey = uiKey;
    applyTheme(currentTheme(), false);
    if (!$("#palette-menu").hidden) renderPaletteMenu();
  }
  $("#title").textContent = d.title;
  $("#foot-info").textContent = `pi-status ${d.version} · ${d.interval}s refresh`;
  renderStatus();
  renderAlerts(d);
  renderBento(d);
  renderServices(d);
  if (S.sheet && S.sheet.kind !== "log") renderSheet(d);
}

async function poll() {
  clearTimeout(S.timer);
  try {
    const r = await fetch("api/status", { cache: "no-store", signal: AbortSignal.timeout(10000) });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const d = await r.json();
    S.offset = d.now - Date.now() / 1000;
    S.data = d;
    S.lastOk = Date.now();
    S.error = null;
    render(d);
  } catch (e) {
    S.error = e.message || String(e);
    renderStatus();
  }
  // Overlapping polls (tab refocus, version refresh) must not each leave a timer behind.
  clearTimeout(S.timer);
  if (!document.hidden) S.timer = setTimeout(poll, Math.max(2, S.data?.interval ?? 5) * 1000);
}

document.addEventListener("visibilitychange", () => {
  if (document.hidden) clearTimeout(S.timer);
  else poll();
});
setInterval(() => { renderClock(); if (S.data) renderStatus(); }, 1000);
renderClock();

/* ============================ interactions ============================ */

const SOURCE = { system: "cpu", power: "power", storage: "storage", bitcoin: "bitcoin", logs: "logs" };

function flash(el) {
  if (!el) return;
  el.scrollIntoView({ behavior: "smooth", block: "center" });
  el.classList.remove("flash");
  void el.offsetWidth;
  el.classList.add("flash");
}

document.addEventListener("click", (e) => {
  const t = e.target;

  const alert = t.closest(".alert");
  if (alert) {
    const src = alert.dataset.src;
    if (src === "services") {
      setFilter("issues");
      flash($("#services-section"));
    } else if (src === "panels") {
      flash($('#bento [data-mod^="panel:"]'));
    } else {
      flash($(`#bento [data-mod="${SOURCE[src] ?? src}"]`));
    }
    return;
  }
  if (t.closest('[data-act="alerts"]')) { S.alertsOpen = !S.alertsOpen; renderAlerts(S.data); return; }
  if (t.closest("#status")) { if (!$("#alerts").hidden) flash($("#alerts")); return; }

  const seg = t.closest(".seg button");
  if (seg) { setFilter(seg.dataset.filter); return; }

  const svc = t.closest("[data-svc]");
  if (svc) { openSheet("svc", svc.dataset.svc); return; }

  const card = t.closest("[data-open]");
  if (card) { const [kind, ...id] = card.dataset.open.split(":"); openSheet(kind, id.join(":")); return; }

  const log = t.closest("[data-log]");
  if (log) { openSheet("log", log.dataset.log); return; }
});

function setFilter(f) {
  S.filter = f;
  store.set("pi-status.filter", f);
  if (S.data) renderServices(S.data);
}

/* ============================ theme & palette ============================ */

/* A palette is a colour pair + alert accent; app.css derives every other shade. */
const BUILTIN_PALETTES = [
  { id: "mono",    name: "Mono",    dark: "#000000", light: "#ffffff", accent_dark: "#ff2f36", accent_light: "#d71921" },
  { id: "crimson", name: "Crimson", dark: "#3a0510", light: "#f6e7d5", accent_dark: "#ffc93c", accent_light: "#9a3f06" },
  { id: "navy",    name: "Navy",    dark: "#0a1431", light: "#e6edff", accent_dark: "#ff6a4d", accent_light: "#b52a12" },
  { id: "forest",  name: "Forest",  dark: "#0b1c13", light: "#ece5d0", accent_dark: "#ff7b47", accent_light: "#a3360a" },
  { id: "amber",   name: "Amber",   dark: "#130c00", light: "#ffc457", accent_dark: "#ff3b30", accent_light: "#a1000f" },
];

/** Black or white, whichever has the higher WCAG contrast on `hex`. */
function onColor(hex) {
  let h = hex.replace("#", "");
  if (h.length === 3) h = [...h].map((c) => c + c).join("");
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  const L = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return (L + 0.05) / 0.05 >= 1.05 / (L + 0.05) ? "#000000" : "#ffffff";
}

function palettes() {
  const list = BUILTIN_PALETTES.map((p) => ({ ...p }));
  for (const c of S.data?.ui?.palettes ?? []) {
    const i = list.findIndex((p) => p.id === c.id);
    if (i >= 0) list[i] = c; else list.push(c);
  }
  return list;
}

function currentPaletteId() {
  return store.get("pi-status.palette", null) ?? S.data?.ui?.palette ?? "mono";
}

function swatch(p) {
  return `<i style="background:${esc(p.dark)}"></i><i style="background:${esc(p.light)}"></i>` +
    `<i style="background:${esc(p.accent_dark)}"></i>`;
}

function applyPalette(id, remember) {
  const list = palettes();
  const found = list.find((x) => x.id === id);
  const p = found ?? list[0];
  const vars = {
    "--A": p.dark, "--B": p.light,
    "--acc-d": p.accent_dark, "--acc-l": p.accent_light,
    "--on-acc-d": onColor(p.accent_dark), "--on-acc-l": onColor(p.accent_light),
  };
  for (const k in vars) document.documentElement.style.setProperty(k, vars[k]);
  if (found) store.set("pi-status.palette.vars", JSON.stringify(vars));
  if (remember) store.set("pi-status.palette", p.id);
  $("#palette-sw").innerHTML = swatch(p);
  $("#palette-name").textContent = p.name;
  const meta = document.querySelector('meta[name="theme-color"]') ?? document.head.appendChild(Object.assign(document.createElement("meta"), { name: "theme-color" }));
  meta.content = getComputedStyle(document.body).backgroundColor;
}

function renderPaletteMenu() {
  const cur = currentPaletteId();
  $("#palette-menu").innerHTML = palettes().map((p) =>
    `<button type="button" class="menu-item" role="menuitemradio" aria-checked="${p.id === cur}" data-palette="${esc(p.id)}">` +
    `<span class="sw">${swatch(p)}</span>${esc(p.name)}<span class="tick"></span></button>`).join("");
}

function togglePaletteMenu(open) {
  const menu = $("#palette-menu");
  open = open ?? menu.hidden;
  if (open === !menu.hidden) return;
  if (open) renderPaletteMenu();
  menu.hidden = !open;
  $("#palette").setAttribute("aria-expanded", String(open));
  if (open) (menu.querySelector('[aria-checked="true"]') ?? menu.firstElementChild)?.focus();
}

$("#palette-menu").addEventListener("keydown", (e) => {
  const items = [...e.currentTarget.querySelectorAll(".menu-item")];
  const i = items.indexOf(document.activeElement);
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  } else if (e.key === "Escape") {
    togglePaletteMenu(false);
    $("#palette").focus();
  }
});

$("#palette").addEventListener("click", (e) => { e.stopPropagation(); togglePaletteMenu(); });
$("#palette-menu").addEventListener("click", (e) => {
  const item = e.target.closest("[data-palette]");
  if (!item) return;
  applyPalette(item.dataset.palette, true);
  togglePaletteMenu(false);
});
document.addEventListener("click", (e) => { if (!e.target.closest(".picker")) togglePaletteMenu(false); });
document.addEventListener("keydown", (e) => { if (e.key === "Escape") togglePaletteMenu(false); });

/* Light/dark: auto → dark → light. A viewer's choice beats the config default. */
const THEMES = ["auto", "dark", "light"];
function currentTheme() {
  return store.get("pi-status.theme", null) ?? S.data?.ui?.theme ?? "auto";
}
function applyTheme(t, remember) {
  if (!THEMES.includes(t)) t = "auto";
  if (t === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = t;
  $("#theme").textContent = t;
  if (remember) store.set("pi-status.theme", t);
  applyPalette(currentPaletteId(), false); // refresh theme-color meta
}
applyTheme(currentTheme(), false);
$("#theme").addEventListener("click", () => {
  applyTheme(THEMES[(THEMES.indexOf(currentTheme()) + 1) % THEMES.length], true);
});

$("#refresh-versions").addEventListener("click", async () => {
  try {
    const r = await fetch("api/versions/refresh", { method: "POST" });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    toast("Checking versions");
    setTimeout(poll, 4000);
  } catch (e) {
    toast(`Failed: ${e.message}`);
  }
});

let toastTimer;
function toast(msg) {
  const el = $("#toast");
  el.textContent = msg;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 2400);
}

/* Sheet controls */
const wrapBtn = $("#log-wrap");
wrapBtn.setAttribute("aria-pressed", store.get("pi-status.wrap", matchMedia("(max-width: 640px)").matches ? "1" : "0") === "1" ? "true" : "false");
wrapBtn.addEventListener("click", () => {
  const on = wrapBtn.getAttribute("aria-pressed") !== "true";
  wrapBtn.setAttribute("aria-pressed", String(on));
  store.set("pi-status.wrap", on ? "1" : "0");
  renderLog();
});
$("#log-filter").addEventListener("input", renderLog);
$("#log-reload").addEventListener("click", loadLog);
$("#sheet-close").addEventListener("click", () => sheet.close());
sheet.addEventListener("click", (e) => { if (e.target === sheet) sheet.close(); });
sheet.addEventListener("close", () => { S.sheet = null; });

poll();
