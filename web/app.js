// The Libroxel website: the app's home screen in a browser. It reads the
// same Hub as the app (games, server lists, friends and accounts), signs in
// the same way (the password is stretched here and never sent), and Play
// opens the installed app through a libroxel:// link.

import * as crypto from './vendor/crypto.js';

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

// The Hub this site talks to. `?hub=http://127.0.0.1:7430` picks another
// one for testing (remembered in this browser; `?hub=` alone forgets it).
const HUB = (() => {
  const official = document.querySelector('meta[name="libroxel-hub"]')?.content || 'https://hub.libroxel.com';
  try {
    const asked = new URLSearchParams(location.search).get('hub');
    if (asked !== null) {
      if (asked) localStorage.setItem('libroxel.hub', asked);
      else localStorage.removeItem('libroxel.hub');
    }
    return (localStorage.getItem('libroxel.hub') || official).replace(/\/+$/, '');
  } catch {
    return official;
  }
})();

// The starter games built into the app (crates/libroxel/src/starter_games.rs).
const STARTER_GAMES = [
  { name: 'Sample obby', symbol: '', about: 'An obby with coins, TNT and a rover' },
  { name: 'Kart race', symbol: '', about: 'Three laps round the track in a kart' },
  { name: 'Coin rush', symbol: '', about: 'Beat a robot to the coins before time runs out' },
];

const ICON = {
  house: '', users: '', play: '', broadcast: '', shirt: '',
  globe: '', left: '', right: '', signIn: '', userPlus: '',
  userMinus: '', check: '', search: '', refresh: '', bell: '',
  download: '', signOut: '', signal: '', shield: '', crown: '',
  seal: '', x: '', lock: '', user: '',
};

// The app's colours, for drawing.
const C = {
  ink0: [18, 17, 15], ink1: [28, 26, 23], ink2: [38, 35, 31], ink3: [52, 48, 42], line: [63, 58, 51],
  tangerine: [255, 106, 43], mint: [47, 212, 154], sky: [89, 168, 255], sun: [255, 201, 60],
  brick: [242, 84, 91], dust: [163, 155, 139],
};
const TINTS = [C.tangerine, C.mint, C.sky, C.sun, [178, 146, 255], C.brick];

// The Avatar tab's colours (crates/libroxel/src/menu.rs) and hats.
const SKINS = [[255, 224, 196], [238, 196, 160], [222, 172, 128], [196, 140, 96], [160, 108, 72], [118, 78, 52], [86, 56, 38], [120, 200, 120]];
const CLOTHES = [[236, 98, 74], [242, 170, 40], [250, 220, 70], [120, 200, 80], [32, 168, 150], [70, 150, 230], [50, 56, 84], [124, 92, 214], [226, 86, 150], [240, 240, 240], [150, 150, 160], [40, 40, 46], [120, 80, 50], [180, 40, 50], [20, 90, 60], [250, 200, 60]];
const SHIRTS = [[236, 98, 74], [124, 92, 214], [32, 168, 150], [242, 170, 40], [226, 86, 150], [70, 150, 230]];
const HATS = ['No hat', 'Cap', 'Tall hat', 'Crown', 'Halo', 'Antenna'];

// The bot check for new accounts, and password stretching, as the Hub asks.
const WORK_BITS = 20;
const PASSWORD_MEMORY_KIB = 64 * 1024;
const PASSWORD_PASSES = 3;
const MIN_PASSWORD = 10;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/** Makes an element. Text always goes in as text, never as HTML. */
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === undefined || value === null || value === false) continue;
    if (key === 'class') node.className = value;
    else if (key.startsWith('on')) node.addEventListener(key.slice(2), value);
    else if (key === 'style') node.style.cssText = value;
    else node.setAttribute(key, value === true ? '' : value);
  }
  add(node, children);
  return node;
}

function add(node, children) {
  for (const child of children.flat(Infinity)) {
    if (child === undefined || child === null || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

const icon = (glyph) => el('i', { class: 'icon', 'aria-hidden': 'true' }, glyph);
const svgNS = 'http://www.w3.org/2000/svg';
function svg(tag, attrs = {}, ...children) {
  const node = document.createElementNS(svgNS, tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  for (const child of children.flat()) if (child) node.append(child);
  return node;
}

const utf8 = (text) => new TextEncoder().encode(text);
const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
function unhex(text) {
  if (!/^([0-9a-f]{2})*$/.test(text)) throw new Error('the Hub sent something that is not hex');
  return Uint8Array.from(text.match(/../g) || [], (b) => parseInt(b, 16));
}
const random = (n) => crypto_random(n);
function crypto_random(n) {
  const bytes = new Uint8Array(n);
  globalThis.crypto.getRandomValues(bytes);
  return bytes;
}
const rgb = ([r, g, b]) => `rgb(${r}, ${g}, ${b})`;
const shade = (c, f) => c.map((v) => Math.min(255, Math.floor(v * f + 0.5)));
const lerp = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
const nowSecs = () => Math.floor(Date.now() / 1000);

/** FNV-1a over the UTF-8 bytes, as the app picks a game's colour and scene. */
function fnv(text) {
  let h = 2166136261;
  for (const b of utf8(text)) h = Math.imul(h ^ b, 16777619) >>> 0;
  return h;
}
const tint = (title) => TINTS[fnv(title) % TINTS.length];

/** The app's seed for a name's usual look (64-bit wrapping arithmetic). */
function seed(name) {
  let h = 7n;
  for (const b of utf8(name)) h = (h * 31n + BigInt(b)) & 0xffffffffffffffffn;
  return h;
}

/** "5 min ago", "3 h ago", "2 days ago". */
function ago(secs) {
  if (secs < 60) return 'just now';
  if (secs < 3600) return `${Math.floor(secs / 60)} min ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)} h ago`;
  const days = Math.floor(secs / 86400);
  return days === 1 ? '1 day ago' : `${days} days ago`;
}

/** 950, 1.2K, 34K. */
function count(n) {
  if (n < 1000) return String(n);
  if (n < 10000) return `${(n / 1000).toFixed(1)}K`;
  return `${Math.floor(n / 1000)}K`;
}

/**
 * JSON exactly as the Hub prints it back when it checks a signature
 * (serde_json: keys in sorted order, no spaces).
 */
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') {
    const keys = Object.keys(value).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

// ---------------------------------------------------------------------------
// The Hub
// ---------------------------------------------------------------------------

class HubError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function api(path, { method = 'GET', body, query } = {}) {
  let url = HUB + path;
  if (query) url += `?${new URLSearchParams(query)}`;
  let response;
  try {
    response = await fetch(url, {
      method,
      headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new HubError(0, `Could not reach the Hub at ${HUB}.`);
  }
  const text = await response.text();
  if (!response.ok) {
    const why = text.trim() || response.statusText || `error ${response.status}`;
    throw new HubError(response.status, why.charAt(0).toUpperCase() + why.slice(1));
  }
  return text ? JSON.parse(text) : null;
}

// ---------------------------------------------------------------------------
// Signing in: the identity key lives in this browser once signed in
// ---------------------------------------------------------------------------

const SESSION_KEY = 'libroxel.session';
let session = loadSession();
let lastSigned = { friends: 0, account: 0 };

function loadSession() {
  for (const store of [sessionStorage, localStorage]) {
    try {
      const saved = JSON.parse(store.getItem(SESSION_KEY) || 'null');
      if (saved?.playerId && saved?.secret) return saved;
    } catch {
      /* storage blocked or bad: signed out */
    }
  }
  return null;
}

function saveSession(next, remember) {
  session = next;
  try {
    sessionStorage.removeItem(SESSION_KEY);
    localStorage.removeItem(SESSION_KEY);
    if (next) (remember ? localStorage : sessionStorage).setItem(SESSION_KEY, JSON.stringify(next));
  } catch {
    /* kept for this page only */
  }
}

const secretKey = () => unhex(session.secret);
const signText = (secret, text) => hex(crypto.sign(secret, utf8(text)));
const playerIdOf = (secret) => hex(crypto.publicKey(secret));

async function stretch(password, saltHex, memoryKib, passes) {
  const out = await crypto.stretch(password, unhex(saltHex), memoryKib, passes);
  return { login: out.slice(0, 32), seal: out.slice(32, 64) };
}

/** Asks the Hub how to stretch `name`'s password, then stretches it. */
async function stretchFor(name, password) {
  const salt = await api(`/accounts/sign-in/${encodeURIComponent(name)}`);
  const safe = salt.memory_kib >= 19 * 1024 && salt.memory_kib <= 1024 * 1024 && salt.passes >= 2 && salt.passes <= 10;
  if (!safe) throw new Error('This Hub asks for unsafe password settings.');
  return stretch(password, salt.salt, salt.memory_kib, salt.passes);
}

/** Signs in with keys from `stretchFor`; `code` is the two-step code, if any. */
async function signIn(name, keys, code) {
  const time = Date.now();
  const signature = signText(keys.login, `libroxel sign in\n${name.toLowerCase()}\n${time}`);
  const signedIn = await api('/accounts/sign-in', { method: 'POST', body: { name, time, signature, code: code || null } });
  const sealed = unhex(signedIn.sealed);
  if (sealed.length < 12) throw new Error('The Hub sent a bad key.');
  let secret;
  try {
    secret = crypto.open(keys.seal, sealed.slice(0, 12), sealed.slice(12), utf8(signedIn.player_id));
  } catch {
    throw new Error('That password does not open this account.');
  }
  if (secret.length !== 32 || playerIdOf(secret) !== signedIn.player_id) throw new Error("The Hub sent someone else's key.");
  return { playerId: signedIn.player_id, secret: hex(secret) };
}

function passwordProblem(password, name) {
  if ([...password].length < MIN_PASSWORD) return `A password is at least ${MIN_PASSWORD} characters.`;
  if (utf8(password).length > 256) return 'That password is too long.';
  if (name && password.toLowerCase().includes(name.toLowerCase())) return "A password can't have your name in it.";
  return null;
}

/** The bot check: a number whose hash starts with WORK_BITS zero bits. */
async function work(playerId, name, progress) {
  const prefix = `libroxel work\n${playerId}\n${name.toLowerCase()}\n`;
  for (let nonce = 0; ; nonce++) {
    const hash = crypto.sha256Bytes(utf8(prefix + nonce));
    let bits = 0;
    for (const byte of hash) {
      if (byte === 0) bits += 8;
      else {
        bits += Math.clz32(byte) - 24;
        break;
      }
    }
    if (bits >= WORK_BITS) return nonce;
    if (nonce % 20000 === 19999) {
      progress?.(nonce);
      await new Promise((r) => setTimeout(r, 0));
    }
  }
}

/** Makes a new account with a password, as the app does, and signs in. */
async function createAccount(name, password, onStep) {
  const secret = random(32);
  const playerId = playerIdOf(secret);
  onStep('Checking you are not a bot…');
  const nonce = await work(playerId, name);
  const look = usualLook(name);
  const time = nowSecs();
  const about = '';
  const signature = signText(secret, `libroxel account\n${playerId}\n${name}\n${about}\n${canonical(look)}\n${time}`);
  onStep('Claiming your name…');
  await api('/accounts', { method: 'POST', body: { player_id: playerId, name, about, look, time, signature, work: nonce } });
  lastSigned.account = time;
  onStep('Locking your account with your password…');
  await setPassword(secret, playerId, password);
  return { playerId, secret: hex(secret), name };
}

async function setPassword(secret, playerId, password) {
  const salt = hex(random(16));
  const keys = await stretch(password, salt, PASSWORD_MEMORY_KIB, PASSWORD_PASSES);
  const nonce = random(12);
  const sealed = hex(new Uint8Array([...nonce, ...crypto.seal(keys.seal, nonce, secret, utf8(playerId))]));
  const set = {
    player_id: playerId, salt, memory_kib: PASSWORD_MEMORY_KIB, passes: PASSWORD_PASSES,
    login_id: playerIdOf(keys.login), sealed, time: Math.max(nowSecs(), lastSigned.account + 1),
  };
  set.signature = signText(secret, `libroxel password\n${set.player_id}\n${set.salt}\n${set.memory_kib}\n${set.passes}\n${set.login_id}\n${set.sealed}\n${set.time}`);
  await api('/accounts/password', { method: 'POST', body: set });
}

// ---------------------------------------------------------------------------
// What the Hub lists, kept fresh while the page is open
// ---------------------------------------------------------------------------

const data = { games: [], servers: [], friends: null, me: null, loaded: false, error: null };

async function refresh() {
  try {
    const [games, servers] = await Promise.all([api('/games'), api('/servers')]);
    data.games = games;
    data.servers = servers;
    data.error = null;
  } catch (e) {
    data.error = e.message;
  }
  if (session) {
    try {
      const time = Date.now();
      const signature = signText(secretKey(), `libroxel friends\n${session.playerId}\n${time}`);
      data.friends = await api(`/friends/${session.playerId}`, { query: { time, signature } });
    } catch (e) {
      data.friends = data.friends || { friends: [], incoming: [], outgoing: [] };
    }
    try {
      data.me = await api(`/accounts/${session.playerId}`);
      if (data.me.name !== session.name) saveSession({ ...session, name: data.me.name }, isRemembered());
    } catch (e) {
      if (e.status === 404) data.me = null;
    }
  } else {
    data.friends = null;
    data.me = null;
  }
  data.loaded = true;
}

function isRemembered() {
  try {
    return localStorage.getItem(SESSION_KEY) !== null;
  } catch {
    return false;
  }
}

/** Every game worth showing, with its live servers, busiest first. */
function entries() {
  const list = STARTER_GAMES.map((g) => ({
    key: `starter:${g.name}`, title: g.name, by: 'Libroxel', about: g.about, symbol: g.symbol,
    source: 'starter', servers: [],
  }));
  for (const game of data.games) {
    list.push({
      key: game.game_id, title: game.title, by: game.author, byId: game.author_id, about: game.description,
      symbol: ICON.globe, source: 'published', game, servers: [],
    });
  }
  for (const server of data.servers) {
    const found = list.find((e) => (server.game_id ? e.key === server.game_id : e.title === server.game));
    if (found) found.servers.push(server);
    else {
      list.push({
        key: server.game_id || `live:${server.game}`, title: server.game, by: 'Someone hosting', about: '',
        symbol: ICON.broadcast, source: 'live', servers: [server],
      });
    }
  }
  const playing = (e) => e.servers.reduce((n, s) => n + s.players, 0);
  for (const e of list) e.playing = playing(e);
  return list
    .map((e, i) => [e, i])
    .sort(([a, i], [b, j]) => b.playing - a.playing || i - j)
    .map(([e]) => e);
}

const friends = () => data.friends?.friends || [];
const friendsIn = (serverId) => friends().filter((f) => f.server?.server_id === serverId);
const full = (s) => s.max_players > 0 && s.players >= s.max_players;
function bestServer(servers) {
  let best = null;
  for (const s of servers) {
    if (full(s)) continue;
    const score = [friendsIn(s.server_id).length, s.players];
    if (!best || score[0] > best.score[0] || (score[0] === best.score[0] && score[1] > best.score[1])) best = { s, score };
  }
  return best?.s || null;
}

// ---------------------------------------------------------------------------
// Pictures: game scenes and avatars, drawn the way the app draws them
// ---------------------------------------------------------------------------

function diorama(width, height, cx, cy, unit, title) {
  const colour = tint(title);
  const hash = fnv(title);
  const [dx, dy] = [unit * 0.866, unit * 0.5];
  const at = (x, y, z) => `${(cx + (x - y) * dx).toFixed(2)},${(cy + (x + y) * dy - z * unit).toFixed(2)}`;
  const poly = (points, fill, stroke) =>
    svg('polygon', { points: points.join(' '), fill: rgb(fill), ...(stroke ? { stroke: rgb(stroke), 'stroke-width': 1 } : {}) });
  const shapes = [];
  const g = (x, y) => at(x, y, 0.6);
  shapes.push(poly([g(-1.7, -1.7), g(1.7, -1.7), g(1.7, 1.7), g(-1.7, 1.7)], C.ink3, C.line));
  const cells = [];
  for (let x = 0; x < 3; x++) for (let y = 0; y < 3; y++) cells.push([x, y]);
  cells.sort((a, b) => a[0] + a[1] - (b[0] + b[1]));
  for (const [gx, gy] of cells) {
    const bits = (hash >>> ((gx * 3 + gy) * 2)) & 3;
    const high = gx === 1 && gy === 1 ? 2 : Math.max(bits - 1, 0);
    for (let z = 0; z < high; z++) {
      const c = z + 1 === high ? colour : lerp(colour, C.ink2, 0.35);
      const [x, y, zz] = [gx - 1.5, gy - 1.5, 0.6 + z];
      const top = [at(x, y, zz + 1), at(x + 1, y, zz + 1), at(x + 1, y + 1, zz + 1), at(x, y + 1, zz + 1)];
      const left = [at(x, y + 1, zz + 1), at(x + 1, y + 1, zz + 1), at(x + 1, y + 1, zz), at(x, y + 1, zz)];
      const right = [at(x + 1, y, zz + 1), at(x + 1, y + 1, zz + 1), at(x + 1, y + 1, zz), at(x + 1, y, zz)];
      shapes.push(poly(left, shade(c, 0.78)), poly(right, shade(c, 0.55)), poly(top, c));
    }
  }
  return svg('svg', { viewBox: `0 0 ${width} ${height}`, class: 'scene', 'aria-hidden': 'true' }, ...shapes);
}

const usualLook = (name) => ({
  hat: 'No hat', hat_color: [240, 200, 60], pants: [50, 56, 84],
  shirt: SHIRTS[Number(seed(name) % BigInt(SHIRTS.length))], skin: [238, 196, 160],
});

/** A look from the Hub, with anything missing taken from the usual one. */
function readLook(look, name) {
  const usual = usualLook(name);
  const colour = (key) => {
    const v = look?.[key];
    return Array.isArray(v) && v.length === 3 && v.every((n) => Number.isInteger(n) && n >= 0 && n <= 255) ? v : usual[key];
  };
  return {
    skin: colour('skin'), shirt: colour('shirt'), pants: colour('pants'), hat_color: colour('hat_color'),
    hat: HATS.includes(look?.hat) ? look.hat : usual.hat,
    item: look?.item,
  };
}

/** A round portrait: head and shoulders in their own colours. */
function portrait(look, name, size) {
  const l = readLook(look, name);
  const r = 50;
  const [cx, cy] = [50, 50];
  const cut = cy + r * 0.42;
  const half = Math.sqrt(Math.max(r * r - (cut - cy) ** 2, 0));
  const start = Math.atan2(cut - cy, half);
  const end = Math.PI - start;
  const pts = [];
  for (let i = 0; i <= 24; i++) {
    const a = start + ((end - start) * i) / 24;
    pts.push(`${(cx + r * Math.cos(a)).toFixed(2)},${(cy + r * Math.sin(a)).toFixed(2)}`);
  }
  const [hw, hh] = [r * 0.86, r * 0.8];
  const [hx, hy] = [cx - hw / 2, cy - r * 0.08 - hh / 2];
  const dark = 'rgb(30, 26, 22)';
  const parts = [
    svg('circle', { cx, cy, r, fill: rgb(C.ink3) }),
    svg('polygon', { points: pts.join(' '), fill: rgb(l.shirt) }),
    svg('rect', { x: hx, y: hy, width: hw, height: hh, rx: r * 0.16, fill: rgb(l.skin) }),
  ];
  for (const side of [-1, 1]) parts.push(svg('circle', { cx: cx + side * r * 0.16, cy: hy + hh / 2 - r * 0.05, r: r * 0.075, fill: dark }));
  parts.push(svg('line', { x1: cx - r * 0.12, x2: cx + r * 0.12, y1: hy + hh / 2 + r * 0.17, y2: hy + hh / 2 + r * 0.17, stroke: dark, 'stroke-width': r * 0.05 }));
  if (l.hat !== 'No hat' || l.item) {
    parts.push(svg('rect', { x: hx, y: hy - r * 0.12, width: hw, height: r * 0.18, rx: r * 0.08, fill: rgb(l.hat_color) }));
  }
  return svg('svg', { viewBox: '0 0 100 100', width: size, height: size, 'aria-hidden': 'true' }, ...parts);
}

/** The whole figure from the front, for the Avatar tab. */
function figure(look, name) {
  const l = readLook(look, name);
  const p = [];
  const box = (x, y, w, h, c, rx = 3) => p.push(svg('rect', { x, y, width: w, height: h, rx, fill: rgb(c) }));
  box(58, 228, 38, 82, shade(l.pants, 0.9)); // legs
  box(100, 228, 38, 82, l.pants);
  box(54, 140, 88, 90, l.shirt); // body
  box(22, 140, 30, 82, l.skin); // arms
  box(144, 140, 30, 82, l.skin);
  box(66, 72, 64, 62, l.skin, 6); // head
  p.push(svg('rect', { x: 82, y: 96, width: 9, height: 12, rx: 2, fill: 'rgb(30, 26, 22)' }));
  p.push(svg('rect', { x: 105, y: 96, width: 9, height: 12, rx: 2, fill: 'rgb(30, 26, 22)' }));
  const hat = l.hat_color;
  if (l.hat === 'Cap') { box(64, 58, 68, 18, hat, 6); box(110, 70, 34, 7, shade(hat, 0.8)); }
  if (l.hat === 'Tall hat') { box(70, 20, 56, 52, hat); box(58, 64, 80, 10, shade(hat, 0.85)); }
  if (l.hat === 'Crown') p.push(svg('polygon', { points: '68,72 68,44 82,58 98,38 114,58 128,44 128,72', fill: rgb(hat) }));
  if (l.hat === 'Halo') p.push(svg('ellipse', { cx: 98, cy: 54, rx: 34, ry: 7, fill: 'none', stroke: rgb(hat), 'stroke-width': 5 }));
  if (l.hat === 'Antenna') { box(96, 40, 5, 34, shade(hat, 0.8), 1); p.push(svg('circle', { cx: 98.5, cy: 38, r: 8, fill: rgb(hat) })); }
  if (l.item && l.hat === 'No hat') box(66, 56, 64, 16, hat, 5);
  return svg('svg', { viewBox: '0 0 196 320', width: 196, height: 320, 'aria-hidden': 'true' }, ...p);
}

function logo() {
  return svg('svg', { viewBox: '0 0 28 30', 'aria-hidden': 'true' },
    svg('polygon', { points: '14,1 27,8 14,15 1,8', fill: rgb([255, 150, 90]) }),
    svg('polygon', { points: '1,8 14,15 14,29 1,22', fill: rgb(C.tangerine) }),
    svg('polygon', { points: '27,8 14,15 14,29 27,22', fill: rgb([200, 78, 26]) }));
}

// ---------------------------------------------------------------------------
// Opening the app
// ---------------------------------------------------------------------------

/**
 * Opens the installed app at `link` (libroxel://…). Browsers say nothing
 * when no app takes the link, so if the page is still in front after a
 * moment, offer the download.
 */
function launch(link, what) {
  const box = el('div', { class: 'dialog', role: 'dialog', 'aria-modal': 'true' });
  const overlay = el('div', { class: 'overlay', onclick: (e) => e.target === overlay && close() }, box);
  const close = () => {
    overlay.remove();
    clearTimeout(timer);
    window.removeEventListener('blur', left);
    document.removeEventListener('visibilitychange', left);
  };
  const show = (...children) => {
    box.replaceChildren(...children);
  };
  show(
    el('div', { class: 'spinner' }),
    el('h2', {}, 'Opening Libroxel…'),
    el('p', {}, what),
    el('div', { class: 'actions' }, el('button', { class: 'btn small', onclick: close }, 'Cancel')),
  );
  let opened = false;
  const left = () => {
    opened = true;
    show(
      el('h2', {}, 'Libroxel is opening'),
      el('p', {}, `${what} If nothing happens, open Libroxel yourself and try again.`),
      el('div', { class: 'actions' }, el('button', { class: 'btn small', onclick: close }, 'Close')),
    );
  };
  window.addEventListener('blur', left);
  document.addEventListener('visibilitychange', left);
  const timer = setTimeout(() => {
    if (opened) return;
    show(
      el('h2', {}, 'Did Libroxel open?'),
      el('p', {}, "If your browser asked, choose Open Libroxel. If nothing happened, you may not have Libroxel yet: download it, open it once, then press Play again."),
      el('div', { class: 'actions' },
        el('a', { class: 'btn go small', href: '#/download', onclick: close }, icon(ICON.download), 'Download Libroxel'),
        el('button', { class: 'btn small', onclick: () => { close(); launch(link, what); } }, 'Try again')),
    );
  }, 2500);
  document.body.append(overlay);
  window.location.href = link;
}

const appLink = (action, value) => `libroxel://${action}/${encodeURIComponent(value)}`;
const joinServer = (server) => launch(appLink('join', server.code), `Joining ${server.name}.`);

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

const page = document.getElementById('page');
let route = { name: 'home' };
let flash = null;

function parseRoute() {
  const parts = location.hash.replace(/^#\/?/, '').split('/').map((p) => {
    try {
      return decodeURIComponent(p);
    } catch {
      return p;
    }
  });
  switch (parts[0]) {
    case 'game': return { name: 'game', key: parts.slice(1).join('/') };
    case 'user': return { name: 'user', id: parts[1] };
    case 'friends': case 'avatar': case 'sign-in': case 'create': case 'download': return { name: parts[0] };
    default: return { name: 'home' };
  }
}

function tabs() {
  const tab = (name, glyph, label, href) =>
    el('a', { class: `tab${route.name === name ? ' active' : ''}`, href }, icon(glyph), label);
  document.getElementById('tabs').replaceChildren(
    tab('home', ICON.house, 'Home', '#/'),
    tab('friends', ICON.users, 'Friends', '#/friends'),
    tab('avatar', ICON.shirt, 'Avatar', '#/avatar'),
    tab('download', ICON.download, 'Download', '#/download'),
  );
  const who = document.getElementById('who');
  if (session) {
    who.replaceChildren(
      el('span', { class: 'caps' }, 'Playing as'),
      el('a', { class: 'name', href: `#/user/${data.me?.number ?? session.playerId}` }, session.name || 'You'),
      el('button', { class: 'chip iconbtn', title: 'Sign out', onclick: signOut }, icon(ICON.signOut)),
    );
  } else {
    who.replaceChildren(
      el('a', { class: 'chip', href: '#/create' }, 'Make an account'),
      el('a', { class: 'btn go small', href: '#/sign-in' }, icon(ICON.signIn), 'Sign in'),
    );
  }
}

function signOut() {
  saveSession(null);
  data.friends = null;
  data.me = null;
  flash = { text: 'Signed out. This browser no longer has your key.' };
  goTo('#/');
}

/** Goes to `hash`, drawing the page once (a hash change draws it by itself). */
function goTo(hash) {
  if (location.hash === hash) render();
  else location.hash = hash;
}

function status(text, kind = '') {
  return el('div', { class: `status ${kind}` }, text);
}

function render() {
  route = parseRoute();
  tabs();
  const views = { home, game, friends: friendsPage, avatar, user, 'sign-in': signInPage, create: createPage, download };
  const view = views[route.name] || home;
  const nodes = [];
  if (flash) {
    nodes.push(status(flash.text, flash.kind || ''));
    flash = null;
  }
  if (data.error && ['home', 'game', 'friends'].includes(route.name)) nodes.push(status(data.error, 'error'));
  page.replaceChildren(...nodes, ...[view()].flat(Infinity).filter(Boolean));
  document.title = `${titleFor(route)} · Libroxel`;
}

function titleFor(r) {
  if (r.name === 'game') return entries().find((e) => e.key === r.key)?.title || 'Game';
  return { home: 'Home', friends: 'Friends', avatar: 'Avatar', user: 'Player', 'sign-in': 'Sign in', create: 'Make an account', download: 'Download' }[r.name] || 'Libroxel';
}

function whereabouts(friend) {
  if (friend.online && friend.game) return [friend.game, 'var(--sky)'];
  if (friend.online) return ['Online', 'var(--mint)'];
  const when = friend.last_online_secs_ago;
  return [when === null || when === undefined ? 'Offline' : `Offline · ${ago(when)}`, 'var(--dust)'];
}

// --- Home -----------------------------------------------------------------

function home() {
  const out = [];
  if (session) {
    const online = friends().filter((f) => f.online).length;
    out.push(el('div', { class: 'hello' },
      portrait(data.me?.look, session.name || '', 64),
      el('div', {},
        el('h1', {}, `Hello, ${session.name || 'there'}!`),
        el('div', { class: 'soft' }, online === 1 ? '1 friend is online.' : `${online} friends are online.`))));
    const list = friends();
    out.push(el('h2', { class: 'section' }, `Friends (${list.length})`, el('a', { href: '#/friends' }, 'See all ', icon(ICON.right))));
    const waiting = data.friends?.incoming?.length || 0;
    if (waiting) {
      out.push(el('a', { class: 'waiting', href: '#/friends' }, icon(ICON.bell), waiting === 1 ? '1 friend request waiting' : `${waiting} friend requests waiting`));
    }
    out.push(el('div', { class: 'friend-row' },
      el('a', { class: 'friend-tile', href: '#/friends' }, el('div', { class: 'add-face' }, icon(ICON.userPlus)), el('b', {}, 'Add friends')),
      list.map((f) => {
        const [where, colour] = whereabouts(f);
        const dot = f.online ? (f.game ? 'var(--sky)' : 'var(--mint)') : null;
        const tile = el('a', { class: 'friend-tile', href: f.server ? '#' : `#/user/${f.number ?? f.player_id}`, title: f.server ? `Join ${f.name}` : f.name },
          el('div', { class: 'face' }, portrait(f.look, f.name, 78), dot && el('span', { class: 'dot', style: `background:${dot}` })),
          el('b', {}, f.name),
          el('small', { style: `color:${colour}` }, where));
        if (f.server) tile.addEventListener('click', (e) => { e.preventDefault(); joinServer(f.server); });
        return tile;
      })));
  } else {
    out.push(el('div', { class: 'hello' },
      el('div', {},
        el('h1', {}, 'Play games people make'),
        el('div', { class: 'soft' }, 'Find a game, press Play, and Libroxel opens straight into it. ',
          el('a', { href: '#/sign-in' }, 'Sign in'), ' to see your friends.'))));
  }
  out.push(el('h2', { class: 'section' }, 'Games'));
  out.push(el('p', { class: 'note' }, 'Pick a game to see its servers, or press Play on its page to jump in.'));
  if (!data.loaded) out.push(el('p', { class: 'soft' }, 'Loading…'));
  out.push(el('div', { class: 'games' }, entries().map(gameCard)));
  return out;
}

function gameCard(entry) {
  const inside = friends().filter((f) => entry.servers.some((s) => s.server_id === f.server?.server_id));
  let meta;
  if (inside.length) meta = el('div', { class: 'meta' }, icon(ICON.users), ` ${count(entry.playing)} playing · ${inside.map((f) => f.name).join(', ')}`);
  else if (entry.playing) meta = el('div', { class: 'meta live' }, icon(ICON.users), ` ${count(entry.playing)} playing`);
  else meta = el('div', { class: 'meta idle' }, `By ${entry.by}`);
  return el('a', { class: 'game', href: `#/game/${encodeURIComponent(entry.key)}` },
    el('div', { class: 'pic' },
      diorama(188, 150, 94, 78, 13, entry.title),
      el('span', { class: 'badge', style: `background:${rgb(tint(entry.title))}` }, icon(entry.symbol)),
      inside.length ? el('span', { class: 'faces' }, inside.slice(0, 3).map((f) => portrait(f.look, f.name, 26))) : null),
    el('h3', {}, entry.title),
    meta);
}

// --- A game's page ----------------------------------------------------------

const view = { hideFull: false };

function game() {
  const entry = entries().find((e) => e.key === route.key);
  const back = el('a', { class: 'back', href: '#/' }, icon(ICON.left), 'Home');
  if (!entry) {
    return [back, data.loaded ? el('div', { class: 'empty' }, "That game isn't on this Hub (any more).") : el('p', { class: 'soft' }, 'Loading…')];
  }
  const best = bestServer(entry.servers);
  const hostable = entry.source !== 'live';
  const stats = [[count(entry.playing), 'Playing'], [String(entry.servers.length), 'Servers']];
  if (entry.game) stats.push([count(entry.game.plays), 'Visits'], [`v${entry.game.version}`, 'Version']);
  const hint = best ? `Joins ${best.name}` : hostable ? 'No servers yet: starts one that friends can join' : 'Nobody is hosting it right now';
  const play = el('button', { class: 'btn go big', disabled: !best && !hostable }, icon(ICON.play), 'Play');
  play.addEventListener('click', () => {
    if (best) joinServer(best);
    else launch(appLink('play', entry.key), `Starting ${entry.title}.`);
  });
  const by = entry.byId ? el('a', { href: `#/user/${entry.byId}` }, `By ${entry.by}`) : el('div', { style: 'color:var(--sky)' }, `By ${entry.by}`);
  const out = [
    back,
    el('div', { class: 'hero' },
      el('div', { class: 'pic' }, diorama(440, 248, 220, 142, 34, entry.title)),
      el('div', {},
        el('h1', {}, entry.title),
        by,
        el('div', { class: 'stats' }, stats.map(([v, l]) => el('div', {}, el('b', {}, v), el('span', { class: 'caps' }, l)))),
        el('div', { class: 'actions' },
          play,
          hostable && el('button', { class: 'btn big', title: 'Host your own server of this game; it shows on this list', onclick: () => launch(appLink('host', entry.key), `Starting a new server of ${entry.title}.`) }, icon(ICON.broadcast), 'New server')),
        el('div', { class: 'hint' }, hint))),
  ];
  if (entry.about) out.push(el('div', { class: 'about' }, el('div', { class: 'caps' }, 'About'), entry.about));

  const hide = el('input', { type: 'checkbox', checked: view.hideFull });
  hide.addEventListener('change', () => { view.hideFull = hide.checked; render(); });
  out.push(el('div', { class: 'servers-head' },
    el('h2', {}, 'Servers'),
    el('label', {}, hide, 'Hide full'),
    el('button', { class: 'chip', onclick: async () => { await refresh(); render(); } }, icon(ICON.refresh), 'Refresh')));
  const shown = entry.servers.filter((s) => !(view.hideFull && full(s))).sort((a, b) => b.players - a.players);
  const withFriends = shown.filter((s) => friendsIn(s.server_id).length);
  const others = shown.filter((s) => !friendsIn(s.server_id).length);
  if (!shown.length) {
    out.push(el('div', { class: 'empty' }, entry.servers.length ? 'Every server is full. Press New server to start one.' : hostable ? 'No one is playing right now. Press Play to start a server friends can join.' : 'No servers right now.'));
  }
  if (withFriends.length) {
    out.push(el('div', { class: 'caps group' }, 'Servers your friends are in'), withFriends.map((s) => serverRow(s, entry)));
  }
  if (others.length) {
    if (withFriends.length) out.push(el('div', { class: 'caps group' }, 'Other servers'));
    out.push(others.map((s) => serverRow(s, entry)));
  }
  return out;
}

function serverRow(server, entry) {
  const inside = friendsIn(server.server_id);
  const isFull = full(server);
  const kind = server.kind || 'player';
  const filled = server.max_players > 0 ? Math.min(1, server.players / server.max_players) : 0;
  const host = server.host
    ? el('span', {}, `Hosted by ${server.host}`, server.admin_host && el('span', { class: 'tag admin', style: 'margin-left:6px' }, 'Admin'), server.host_verified && el('span', { class: 'tag verified', style: 'margin-left:6px' }, 'Verified'))
    : null;
  return el('div', { class: `server${inside.length ? ' friends' : ''}` },
    el('div', { class: 'mini' }, diorama(52, 52, 26, 30, 6, entry.title)),
    el('div', { class: 'what' },
      el('b', {}, server.name),
      el('small', {},
        el('span', { class: `kind-${kind}` }, `${kind.charAt(0).toUpperCase()}${kind.slice(1)} server`),
        host,
        inside.map((f) => el('span', { style: 'color:var(--sky);display:inline-flex;gap:4px;align-items:center' }, portrait(f.look, f.name, 16), `${f.name} is here`)))),
    el('div', { class: `fill${isFull ? ' full' : ''}` },
      server.max_players ? `${server.players} / ${server.max_players} players` : `${server.players} playing`,
      el('div', { class: 'bar' }, el('i', { style: `width:${server.max_players ? filled * 100 : 0}%` }))),
    el('button', { class: 'btn go small', disabled: isFull, onclick: () => joinServer(server) }, icon(ICON.signIn), isFull ? 'Full' : 'Join'));
}

// --- Friends ----------------------------------------------------------------

let search = { text: '', results: null, busy: false };

async function changeFriend(otherId, action, done) {
  const time = Math.max(Date.now(), lastSigned.friends + 1);
  lastSigned.friends = time;
  const signature = signText(secretKey(), `libroxel friend\n${session.playerId}\n${otherId}\n${action}\n${time}`);
  try {
    await api('/friends', { method: 'POST', body: { player_id: session.playerId, other_id: otherId, action, time, signature } });
    flash = { text: done };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  await refresh();
  render();
}

function needSignIn(why) {
  return [
    el('h1', { class: 'title' }, titleFor(route)),
    el('p', { class: 'note' }, why),
    el('div', { class: 'form-actions' },
      el('a', { class: 'btn go', href: '#/sign-in' }, icon(ICON.signIn), 'Sign in'),
      el('a', { href: '#/create' }, 'or make an account')),
  ];
}

function friendsPage() {
  if (!session) return needSignIn('Sign in to see your friends, where they are playing, and join them.');
  const f = data.friends || { friends: [], incoming: [], outgoing: [] };
  const online = f.friends.filter((x) => x.online).length;
  const left = [el('h2', { class: 'subhead' }, `Your friends (${f.friends.length}) · ${online} online`)];
  if (!f.friends.length) left.push(el('div', { class: 'empty' }, 'No friends yet. Find people by name on the right.'));
  for (const friend of f.friends) {
    const [where, colour] = whereabouts(friend);
    const text = friend.online && friend.game ? `Playing ${friend.game}${friend.server ? ` on ${friend.server.name}` : ''}` : where;
    left.push(el('div', { class: 'person' },
      portrait(friend.look, friend.name, 44),
      el('div', { class: 'what' }, el('a', { href: `#/user/${friend.number ?? friend.player_id}` }, el('b', {}, friend.name)), el('small', { style: `color:${colour}` }, text)),
      el('div', { class: 'buttons' },
        friend.server && el('button', { class: 'btn go small', onclick: () => joinServer(friend.server) }, icon(ICON.signIn), 'Join'),
        el('button', {
          class: 'chip iconbtn', title: `Unfriend ${friend.name}`,
          onclick: (e) => {
            const b = e.currentTarget;
            if (b.dataset.sure) changeFriend(friend.player_id, 'remove', `You and ${friend.name} are no longer friends.`);
            else { b.dataset.sure = '1'; b.replaceChildren(icon(ICON.userMinus), ' Unfriend?'); }
          },
        }, icon(ICON.userMinus)))));
  }

  const right = [];
  if (f.incoming.length) {
    right.push(el('h2', { class: 'subhead sun' }, `Friend requests (${f.incoming.length})`));
    for (const p of f.incoming) {
      right.push(el('div', { class: 'person' },
        portrait(null, p.name, 44),
        el('div', { class: 'what' }, el('a', { href: `#/user/${p.number ?? p.player_id}` }, el('b', {}, p.name)), el('small', { class: 'soft' }, 'Wants to be your friend')),
        el('div', { class: 'buttons' },
          el('button', { class: 'btn go small', onclick: () => changeFriend(p.player_id, 'add', `You and ${p.name} are friends now.`) }, icon(ICON.check), 'Accept'),
          el('button', { class: 'chip', onclick: () => changeFriend(p.player_id, 'remove', `Declined ${p.name}'s request.`) }, 'Decline'))));
    }
  }
  right.push(el('h2', { class: 'subhead', style: 'margin-top:8px' }, 'Add friends'));
  const box = el('input', { type: 'search', placeholder: 'Their name', value: search.text });
  const find = async () => {
    search = { text: box.value.trim(), results: null, busy: true };
    if (!search.text) { search.busy = false; render(); return; }
    render();
    try {
      search.results = await api('/accounts', { query: { q: search.text } });
    } catch (e) {
      flash = { text: e.message, kind: 'error' };
      search.results = [];
    }
    search.busy = false;
    render();
  };
  box.addEventListener('keydown', (e) => e.key === 'Enter' && find());
  right.push(el('div', { class: 'search' }, box, el('button', { class: 'chip', onclick: find }, icon(ICON.search), 'Find')));
  if (search.busy) right.push(el('p', { class: 'soft' }, 'Looking…'));
  if (search.results) {
    const people = search.results.filter((p) => p.player_id !== session.playerId);
    if (!people.length) right.push(el('p', { class: 'soft' }, `No one called “${search.text}” on this Hub.`));
    for (const p of people) {
      const isFriend = f.friends.some((x) => x.player_id === p.player_id);
      const asked = f.outgoing.some((x) => x.player_id === p.player_id);
      const asking = f.incoming.some((x) => x.player_id === p.player_id);
      let action;
      if (isFriend) action = el('span', { class: 'soft' }, 'Friends');
      else if (asked) action = el('button', { class: 'chip', onclick: () => changeFriend(p.player_id, 'remove', `Took back your request to ${p.name}.`) }, 'Cancel');
      else action = el('button', { class: 'btn go small', onclick: () => changeFriend(p.player_id, 'add', asking ? `You and ${p.name} are friends now.` : `Friend request sent to ${p.name}.`) }, icon(ICON.userPlus), asking ? 'Accept' : 'Add');
      right.push(el('div', { class: 'person' },
        portrait(null, p.name, 44),
        el('div', { class: 'what' }, el('a', { href: `#/user/${p.number ?? p.player_id}` }, el('b', {}, p.name)), el('small', { class: asked ? '' : 'soft', style: asked ? 'color:var(--sky)' : '' }, asked ? 'Request sent' : p.about || '')),
        el('div', { class: 'buttons' }, action)));
    }
  }
  if (f.outgoing.length) {
    right.push(el('div', { class: 'caps group' }, 'Requests you sent'));
    for (const p of f.outgoing) {
      right.push(el('div', { class: 'person' },
        portrait(null, p.name, 44),
        el('div', { class: 'what' }, el('b', {}, p.name), el('small', { class: 'soft' }, 'Waiting for them to accept')),
        el('div', { class: 'buttons' }, el('button', { class: 'chip', onclick: () => changeFriend(p.player_id, 'remove', `Took back your request to ${p.name}.`) }, 'Cancel'))));
    }
  }
  return [el('h1', { class: 'title' }, 'Friends'), el('div', { class: 'columns' }, el('div', {}, left), el('div', {}, right))];
}

// --- A player's page --------------------------------------------------------

const profiles = new Map();

function user() {
  const id = route.id;
  const back = el('a', { class: 'back', href: '#/' }, icon(ICON.left), 'Home');
  const known = profiles.get(id);
  if (!known) {
    api(`/accounts/${encodeURIComponent(id)}`)
      .then((p) => profiles.set(id, p))
      .catch(() => profiles.set(id, { missing: true }))
      .finally(() => route.name === 'user' && route.id === id && render());
    return [back, el('p', { class: 'soft' }, 'Loading…')];
  }
  if (known.missing) return [back, el('div', { class: 'empty' }, 'There is no one with that id on this Hub.')];
  const p = known;
  const isMe = session?.playerId === p.player_id;
  const f = data.friends;
  let action = null;
  if (session && !isMe && f) {
    if (f.friends.some((x) => x.player_id === p.player_id)) action = el('span', { class: 'soft' }, icon(ICON.check), ' Friends');
    else if (f.outgoing.some((x) => x.player_id === p.player_id)) action = el('span', { style: 'color:var(--sky)' }, 'Request sent');
    else action = el('button', { class: 'btn go small', onclick: () => changeFriend(p.player_id, 'add', `Friend request sent to ${p.name}.`).then(() => profiles.delete(id)) }, icon(ICON.userPlus), 'Add friend');
  }
  const days = Math.floor(p.joined_secs_ago / 86400);
  return [
    back,
    el('div', { class: 'hello' },
      portrait(p.look, p.name, 84),
      el('div', {},
        el('h1', {}, p.name, ' ', p.admin && el('span', { class: 'tag admin' }, 'Admin'), ' ', p.verified && el('span', { class: 'tag verified' }, 'Verified')),
        el('div', { class: 'soft' }, `#${p.number ?? 0} · `, days < 1 ? 'Joined today' : `Joined ${ago(p.joined_secs_ago)}`),
        action && el('div', { style: 'margin-top:8px' }, action))),
    p.about && el('div', { class: 'about' }, p.about),
    el('h2', { class: 'section' }, `Games (${p.games.length})`),
    p.games.length
      ? el('div', { class: 'games' }, p.games.map((g) => gameCard(entries().find((e) => e.key === g.game_id) || {
        key: g.game_id, title: g.title, by: g.author, servers: [], playing: 0, symbol: ICON.globe,
      })))
      : el('p', { class: 'soft' }, isMe ? 'Games you publish from the app show up here.' : 'No published games yet.'),
  ];
}

// --- Avatar -----------------------------------------------------------------

let draft = null;

function avatar() {
  if (!session) return needSignIn('Sign in to change how you look in every game you play.');
  if (!data.me) return [el('h1', { class: 'title' }, 'Your avatar'), el('p', { class: 'soft' }, data.loaded ? 'Your account could not be loaded.' : 'Loading…')];
  if (!draft || draft.for !== session.playerId) {
    draft = { for: session.playerId, look: readLook(data.me.look, data.me.name), about: data.me.about || '', message: null };
  }
  const look = draft.look;
  const row = (label, key, choices) => el('div', {},
    el('div', {}, label),
    el('div', { class: 'swatches' }, choices.map((c) => el('button', {
      class: `swatch${look[key].join() === c.join() ? ' on' : ''}`, style: `background:${rgb(c)}`, title: rgb(c),
      onclick: () => { look[key] = c; draft.message = null; render(); },
    }))));
  const about = el('textarea', { rows: 3, maxlength: 2000 }, draft.about);
  about.addEventListener('input', () => { draft.about = about.value; });
  const save = el('button', { class: 'btn go', onclick: () => saveAccount(save) }, icon(ICON.check), 'Save');
  return [
    el('h1', { class: 'title' }, 'Your avatar'),
    el('p', { class: 'note' }, 'How you look in every game you play or join. Other players see it too.'),
    el('div', { class: 'avatar-page' },
      el('div', { class: 'figure' }, figure(look, data.me.name), el('div', {}, look.hat)),
      el('div', { style: 'flex:1;min-width:280px' },
        row('Skin', 'skin', SKINS),
        row('Shirt', 'shirt', CLOTHES),
        row('Trousers', 'pants', CLOTHES),
        el('div', {}, 'Hat'),
        el('div', { class: 'hats' }, HATS.map((h) => el('button', { class: `chip${look.hat === h ? ' on' : ''}`, onclick: () => { look.hat = h; delete look.item; render(); } }, h))),
        (look.hat !== 'No hat' || look.item) && row('Hat colour', 'hat_color', CLOTHES),
        el('label', { class: 'field' }, el('span', {}, 'About you'), about),
        el('div', { class: 'form-actions' },
          save,
          el('button', { class: 'chip', onclick: () => { draft.look = usualLook(data.me.name); render(); } }, 'Back to the usual look')),
        draft.message && status(draft.message.text, draft.message.kind))),
    security(),
  ];
}

async function saveAccount(button) {
  button.disabled = true;
  const look = { ...draft.look };
  if (!look.item) delete look.item;
  const name = data.me.name;
  const about = draft.about;
  const time = Math.max(nowSecs(), lastSigned.account + 1);
  lastSigned.account = time;
  const signature = signText(secretKey(), `libroxel account\n${session.playerId}\n${name}\n${about}\n${canonical(look)}\n${time}`);
  try {
    await api('/accounts', { method: 'POST', body: { player_id: session.playerId, name, about, look, time, signature } });
    draft.message = { text: 'Saved. You look like this in every game now.' };
    await refresh();
    draft.look = readLook(data.me.look, name);
  } catch (e) {
    draft.message = { text: e.message, kind: 'error' };
  }
  render();
}

let securityInfo = null;

function security() {
  const panel = el('div', { class: 'panel', style: 'margin-top:26px' }, el('h3', {}, icon(ICON.shield), ' Your account'));
  if (!securityInfo || securityInfo.for !== session.playerId) {
    securityInfo = { for: session.playerId, loading: true };
    const time = Date.now();
    const signature = signText(secretKey(), `libroxel security\n${session.playerId}\n${time}`);
    api(`/accounts/${session.playerId}/security`, { query: { time, signature } })
      .then((s) => { securityInfo = { for: session.playerId, ...s }; })
      .catch((e) => { securityInfo = { for: session.playerId, error: e.message }; })
      .finally(() => route.name === 'avatar' && render());
  }
  if (securityInfo.loading) return add(panel, [el('p', { class: 'soft' }, 'Loading…')]);
  if (securityInfo.error) return add(panel, [status(securityInfo.error, 'error')]);
  add(panel, [
    el('p', { class: 'soft' }, `You are ${session.name} on ${HUB.replace(/^https?:\/\//, '')}. `,
      securityInfo.has_password ? 'You have a password, so you can sign in anywhere.' : 'Set a password in the app to sign in on other computers.',
      ' ', securityInfo.two_step ? 'Two-step sign-in is on.' : 'Turn on two-step sign-in in the app, on its Avatar tab, for a code on top of your password.'),
    securityInfo.sign_ins?.length
      ? el('div', {}, el('div', { class: 'caps' }, 'Latest sign-ins'), el('ul', { class: 'signins' }, securityInfo.sign_ins.slice(0, 6).map((s) => el('li', {}, `${ago(s.secs_ago)} from ${s.from}`))))
      : null,
  ]);
  return panel;
}

// --- Sign in, make an account -------------------------------------------------

let pending = null;
// What the other tab should start with, when one sends you to the other.
let carry = null;

/** The two tabs at the top of signing in and making an account. */
function accountTabs(current) {
  const tab = (name, href, label) => el('a', { class: `chip${current === name ? ' on' : ''}`, href }, label);
  return el('div', { class: 'hats', style: 'margin:4px 0 18px' }, tab('sign-in', '#/sign-in', 'Sign in'), tab('create', '#/create', 'Make an account'));
}

/** Signed in already: say so, and offer to sign out to use another account. */
function signedInAlready(next) {
  return [
    el('h1', { class: 'title' }, next === 'create' ? 'Make an account' : 'Sign in'),
    el('p', {}, `You are signed in as ${session.name}.`),
    el('div', { class: 'form-actions' },
      el('button', {
        class: 'btn go', onclick: () => { saveSession(null); data.friends = null; data.me = null; render(); },
      }, icon(ICON.signOut), next === 'create' ? 'Sign out and make another account' : 'Sign out and sign in as someone else'),
      el('a', { href: '#/' }, 'Go home')),
  ];
}

/** Whether an account called `name` is on the Hub (names ignore case). */
async function accountNamed(name) {
  try {
    return await api(`/accounts/${encodeURIComponent(name)}`);
  } catch (e) {
    if (e.status === 404) return null;
    throw e;
  }
}

function signInPage() {
  if (session) return signedInAlready('sign-in');
  const name = el('input', { type: 'text', autocomplete: 'username', maxlength: 20, value: pending?.name || carry?.name || '' });
  const password = el('input', { type: 'password', autocomplete: 'current-password' });
  const code = el('input', { type: 'text', inputmode: 'numeric', autocomplete: 'one-time-code', maxlength: 8, placeholder: '123456' });
  const remember = el('input', { type: 'checkbox', checked: true });
  const message = el('div');
  const go = el('button', { class: 'btn go', type: 'submit' }, icon(ICON.signIn), 'Sign in');
  const needsCode = Boolean(pending?.keys);
  carry = null;
  const form = el('form', { class: 'narrow' },
    !needsCode && el('label', { class: 'field' }, el('span', {}, 'Name'), name),
    !needsCode && el('label', { class: 'field' }, el('span', {}, 'Password'), password),
    needsCode && el('p', {}, `Signing in as ${pending.name}. `, el('button', { class: 'link', type: 'button', onclick: () => { pending = null; render(); } }, 'Not you?')),
    needsCode && el('label', { class: 'field' }, el('span', {}, 'The code from your authenticator app'), code),
    el('label', { class: 'check' }, remember, 'Keep me signed in on this computer'),
    el('div', { class: 'form-actions' }, go),
    message);
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    go.disabled = true;
    const say = (...text) => message.replaceChildren(el('div', { class: 'status wait' }, ...text));
    const fail = (...text) => message.replaceChildren(el('div', { class: 'status error' }, ...text));
    try {
      if (!pending?.keys) {
        const n = name.value.trim();
        if (!n || !password.value) throw new Error('Type your name and password.');
        say('Checking your password… (this takes a moment, on purpose)');
        pending = { name: n, keys: await stretchFor(n, password.value) };
      } else say('Signing in…');
      const signed = await signIn(pending.name, pending.keys, needsCode ? code.value.trim() : null);
      const who = (await accountNamed(pending.name).catch(() => null))?.name || pending.name;
      saveSession({ ...signed, name: who }, remember.checked);
      pending = null;
      flash = { text: `Signed in. Welcome back, ${who}!` };
      await refresh();
      goTo('#/');
    } catch (err) {
      if (err.status === 401 && pending?.keys && !needsCode) {
        render();
        return;
      }
      if (err.status === 404 || (err.status === 403 && !needsCode)) pending = null;
      go.disabled = false;
      if (err.status !== 404) return fail(err.message);
      // No password under that name: either there's no such account yet, or it was made in the app without one.
      const n = name.value.trim();
      const existing = await accountNamed(n).catch(() => null);
      if (existing) {
        return fail(`${existing.name} has no password yet, so it can only be used on the computer it was made on. Open Libroxel there and set a password on the Avatar tab, then sign in here.`);
      }
      carry = { name: n, password: password.value };
      location.hash = '#/create';
    }
  });
  return [
    el('h1', { class: 'title' }, 'Sign in'),
    accountTabs('sign-in'),
    el('p', { class: 'note' }, 'Use the name and password from the app. New here? Type the name you want and a password, and you can make the account next. Your password never leaves this computer: it is turned into a key here, the same way the app does it.'),
    form,
  ];
}

function createPage() {
  if (session) return signedInAlready('create');
  const from = carry;
  carry = null;
  const name = el('input', { type: 'text', autocomplete: 'username', maxlength: 20, value: from?.name || '' });
  const password = el('input', { type: 'password', autocomplete: 'new-password' });
  if (from?.password) password.value = from.password;
  const again = el('input', { type: 'password', autocomplete: 'new-password' });
  const remember = el('input', { type: 'checkbox', checked: true });
  const message = el('div', {}, from && status(`There's no account called ${from.name} yet. Type the password again to make it.`, 'wait'));
  const go = el('button', { class: 'btn go', type: 'submit' }, icon(ICON.userPlus), 'Make my account');
  const form = el('form', { class: 'narrow' },
    el('label', { class: 'field' }, el('span', {}, 'Name (3 to 20 letters, digits or _, with a letter)'), name),
    el('label', { class: 'field' }, el('span', {}, `Password (at least ${MIN_PASSWORD} characters, without your name in it)`), password),
    el('label', { class: 'field' }, el('span', {}, 'Password again'), again),
    el('label', { class: 'check' }, remember, 'Keep me signed in on this computer'),
    el('div', { class: 'form-actions' }, go),
    message);
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const say = (text, kind = 'wait') => message.replaceChildren(status(text, kind));
    const n = name.value.trim();
    const problem = !/^[A-Za-z0-9_]{3,20}$/.test(n) || !/[A-Za-z]/.test(n)
      ? 'A name is 3 to 20 letters, digits or underscores, with at least one letter.'
      : passwordProblem(password.value, n) || (password.value !== again.value ? "The passwords don't match." : null);
    if (problem) return say(problem, 'error');
    go.disabled = true;
    try {
      say('Checking the name is free…');
      const existing = await accountNamed(n);
      if (existing) throw new Error(`Someone already has the name ${existing.name} (names ignore capitals). If it's yours, sign in instead.`);
      const made = await createAccount(n, password.value, say);
      saveSession(made, remember.checked);
      flash = { text: `Welcome, ${n}! Sign in to the app with the same name and password to play as you.` };
      await refresh();
      goTo('#/');
    } catch (err) {
      say(err.message, 'error');
      go.disabled = false;
    }
  });
  return [
    el('h1', { class: 'title' }, 'Make an account'),
    accountTabs('create'),
    el('p', { class: 'note' }, 'One account for the website and the app: your name, friends and avatar go everywhere with you. Any free name works, Admin too. Already playing in the app? Sign in with the name and password you set there instead.'),
    form,
  ];
}

// --- Download -----------------------------------------------------------------

function platform() {
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return 'windows';
  if (/Mac OS X|Macintosh/i.test(ua) && !/iPhone|iPad/i.test(ua)) return 'macos';
  if (/Linux|X11/i.test(ua) && !/Android/i.test(ua)) return 'linux';
  return null;
}

const REPO = (document.querySelector('meta[name="libroxel-repo"]')?.content || '').trim();
let release = null; // The newest release's files, once GitHub has said.

/** Asks GitHub for the newest release's builds, once. */
function loadRelease() {
  if (release || !/^[\w.-]+\/[\w.-]+$/.test(REPO)) return;
  release = { loading: true };
  fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: 'application/vnd.github+json' } })
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(r.status === 404 ? 'none' : `GitHub answered ${r.status}`))))
    .then((r) => { release = { tag: r.tag_name, assets: r.assets || [] }; })
    .catch((e) => { release = { error: e.message }; })
    .finally(() => route.name === 'download' && render());
}

function download() {
  const mine = platform();
  loadRelease();
  const builds = [
    { id: 'windows', name: 'Windows', file: 'libroxel-windows-x86_64.zip', ends: '-windows-x86_64.zip', about: 'Windows 10 or 11. Unzip and open libroxel.exe. If SmartScreen warns you, choose More info, then Run anyway.' },
    { id: 'macos', name: 'macOS', file: 'libroxel-macos-universal.zip', ends: '-macos-universal.zip', about: 'macOS 11 or newer. Unzip, then right-click Libroxel.app and choose Open the first time.' },
    { id: 'linux', name: 'Linux', file: 'libroxel-linux-x86_64.tar.gz', ends: '-linux-x86_64.tar.gz', about: 'x86-64 with Vulkan. Unpack it and run ./libroxel once, so Play links on this site open it.' },
  ].sort((a, b) => (b.id === mine) - (a.id === mine));
  // From the repository's newest release when there is one, else from this server.
  const link = (b) => {
    if (!REPO) return `downloads/${b.file}`;
    const asset = release?.assets?.find((a) => a.name.startsWith('libroxel-') && a.name.endsWith(b.ends));
    return asset ? asset.browser_download_url : null;
  };
  const out = [
    el('h1', { class: 'title' }, 'Download Libroxel'),
    el('p', { class: 'note' }, 'Libroxel is free. Get the app to play, make and host games.',
      release?.tag ? ` The newest version is ${release.tag.replace(/^v/, '')}.` : ''),
  ];
  if (REPO && release?.loading) out.push(el('p', { class: 'soft' }, 'Looking for the newest version…'));
  if (REPO && release?.error) {
    out.push(status(release.error === 'none' ? 'There is no release to download yet.' : `Could not ask GitHub for the newest version (${release.error}).`, 'wait'));
    out.push(el('p', {}, el('a', { href: `https://github.com/${REPO}/releases` }, 'See every release on GitHub')));
  }
  out.push(el('div', { class: 'downloads' }, builds.map((b) => {
    const href = link(b);
    return el('div', { class: `download${b.id === mine ? ' mine' : ''}` },
      el('h3', {}, b.name, b.id === mine ? el('span', { class: 'caps', style: 'margin-left:8px;color:var(--mint)' }, 'Your computer') : null),
      el('p', {}, b.about),
      href
        ? el('a', { class: `btn small${b.id === mine ? ' go' : ''}`, href }, icon(ICON.download), 'Download')
        : el('button', { class: 'btn small', disabled: true }, icon(ICON.download), REPO && release?.loading ? 'Download' : 'Not out yet'));
  })));
  out.push(
    el('h2', { class: 'section' }, 'Then press Play here'),
    el('ol', { class: 'steps' },
      el('li', {}, 'Open Libroxel once after downloading it. That tells your computer that Play buttons on this site open Libroxel.'),
      el('li', {}, 'Come back here, pick a game, and press Play. Your browser may ask whether to open Libroxel: say yes (and tick Always allow, if it offers).'),
      el('li', {}, 'Sign in to the app with the same name and password as here, so your friends see you.')),
  );
  return out;
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

window.addEventListener('hashchange', () => {
  window.scrollTo(0, 0);
  render();
});
render();
refresh().then(render);
setInterval(() => {
  // Keep lists fresh while someone looks, but don't redraw under their typing.
  if (document.hidden || ['sign-in', 'create', 'avatar'].includes(route.name)) return;
  if (document.activeElement && ['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)) return;
  refresh().then(render);
}, 20000);
document.getElementById('hub-name').textContent = HUB.replace(/^https?:\/\//, '');
