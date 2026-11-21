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
  { name: 'Kart race', symbol: '', about: 'Three laps round the track in a kart' },
  { name: 'Checkers', symbol: '', about: 'Checkers against the computer or anyone in the server' },
  { name: 'Connect 4', symbol: '', about: 'Line up four discs before the other player does' },
  { name: 'Kings in the Corner', symbol: '', about: 'Be first to play every card. For 2 to 4 players' },
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
  portrait: [190, 226, 250],
};

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
const nowSecs = () => Math.floor(Date.now() / 1000);

/** FNV-1a over the UTF-8 bytes, as the app makes a game's picture. */
function fnv(text) {
  let h = 2166136261;
  for (const b of utf8(text)) h = Math.imul(h ^ b, 16777619) >>> 0;
  return h;
}

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
    throw new HubError(0, 'Could not reach the Hub. Check your connection and try again.');
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
  if (!safe) throw new Error('The Hub asks for unsafe password settings.');
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

/** Why `password` can't be used, if it can't. Call `loadCommonPasswords` first. */
function passwordProblem(password, name) {
  if ([...password].length < MIN_PASSWORD) return `A password is at least ${MIN_PASSWORD} characters.`;
  if (utf8(password).length > 256) return 'That password is too long.';
  if (name && password.toLowerCase().includes(name.toLowerCase())) return "A password can't have your name in it.";
  return weakPassword(password);
}

// The most used passwords, the same list the app has built in.
let commonPasswords = null;

async function loadCommonPasswords() {
  if (commonPasswords) return commonPasswords;
  let text;
  try {
    const response = await fetch('common-passwords.txt');
    if (!response.ok) throw new Error();
    text = await response.text();
  } catch {
    throw new Error('Could not load the password check. Try again in a moment.');
  }
  commonPasswords = new Set(text.split('\n').map((l) => l.trim()).filter(Boolean));
  return commonPasswords;
}

// Runs of keys people type as a password, checked both ways round.
const KEY_RUNS = [
  '01234567890',
  'abcdefghijklmnopqrstuvwxyz',
  'qwertyuiopasdfghjklzxcvbnm',
  '1qaz2wsx3edc4rfv5tgb6yhn7ujm8ik9ol0p',
  'qazwsxedcrfvtgbyhnujmikolp',
  "zaq12wsxcde34rfvbgt56yhnmju78ik,.lo90p;/-['=]",
];

function unleet(word, one) {
  const look = { 0: 'o', 1: one, '!': one, '|': one, 3: 'e', 4: 'a', '@': 'a', 5: 's', $: 's', 7: 't', '+': 't', 8: 'b', 9: 'g' };
  return [...word].map((c) => look[c] || c).join('');
}

const trimNonLetters = (word) => word.replace(/^\P{L}+|\P{L}+$/gu, '');

/** Why `password` is too easy to guess, if it is (as the app checks it). */
function weakPassword(password) {
  const common = commonPasswords || new Set();
  const lower = password.toLowerCase();
  if (common.has(lower)) return 'That is one of the most common passwords.';
  const chars = [...lower];
  if (new Set(chars).size < 5) return 'That password uses too few different characters.';
  const n = chars.length;
  for (let k = 1; k <= n / 2; k++) {
    if (n % k === 0 && lower === chars.slice(0, k).join('').repeat(n / k)) return 'That password is the same few characters over and over.';
  }
  const reversed = chars.reverse().join('');
  if (KEY_RUNS.some((run) => run.includes(lower) || run.includes(reversed))) return 'That password is a run of keys.';
  for (const word of [trimNonLetters(lower), lower]) {
    for (const one of ['i', 'l']) {
      const plain = unleet(word, one);
      for (const guess of [plain, trimNonLetters(plain)]) {
        if ([...guess].length >= 4 && common.has(guess)) return 'That is a common password with a few changes.';
      }
    }
  }
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
async function createAccount(name, password, email, onStep) {
  const secret = random(32);
  const playerId = playerIdOf(secret);
  onStep('Checking you are not a bot…');
  const nonce = await work(playerId, name);
  const look = usualLook(name);
  const time = nowSecs();
  const about = '';
  const signature = signText(secret, `libroxel account\n${playerId}\n${name}\n${about}\n${canonical(look)}\n${time}`);
  onStep('Claiming your name…');
  await api('/accounts', { method: 'POST', body: { player_id: playerId, name, about, look, time, signature, work: nonce, email } });
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

/**
 * A game's picture from its id, the way GitHub makes default profile
 * pictures: a 5×5 grid of blocks mirrored left to right, in one colour.
 * The app makes the same one (`blocks` in crates/libroxel/src/menu.rs).
 */
function blocks(id) {
  let h = fnv(id);
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b) >>> 0;
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35) >>> 0;
  h = (h ^ (h >>> 16)) >>> 0;
  let bits = h & 0x7fff;
  let ones = 0;
  for (let i = 0; i < 15; i++) ones += (bits >>> i) & 1;
  if (ones < 5) bits ^= 0x7fff;
  const cells = new Array(25).fill(false);
  for (let i = 0; i < 15; i++) {
    if ((bits >>> i) & 1) {
      const [col, row] = [Math.floor(i / 5), i % 5];
      cells[row * 5 + col] = true;
      cells[row * 5 + 4 - col] = true;
    }
  }
  return { cells, colour: hsl((h >>> 15) % 360, 0.6, 0.45 + ((h >>> 28) / 15) * 0.1) };
}

function hsl(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const [r, g, b] = [[c, x, 0], [x, c, 0], [0, c, x], [0, x, c], [x, 0, c], [c, 0, x]][Math.min(Math.floor(h / 60), 5)];
  const m = l - c / 2;
  return [r, g, b].map((v) => Math.round((v + m) * 255));
}

const pictureColour = (id) => blocks(id).colour;

/** A game's picture: its blocks on light grey in the middle, a block of margin round them. */
function gamePicture(width, height, id) {
  const { cells, colour } = blocks(id);
  const cell = Math.min(width, height) / 7;
  const [x0, y0] = [(width - cell * 5) / 2, (height - cell * 5) / 2];
  const shapes = [svg('rect', { width, height, fill: 'rgb(240, 240, 240)' })];
  cells.forEach((on, i) => {
    if (on) shapes.push(svg('rect', { x: x0 + cell * (i % 5), y: y0 + cell * Math.floor(i / 5), width: cell, height: cell, fill: rgb(colour) }));
  });
  return svg('svg', { viewBox: `0 0 ${width} ${height}`, class: 'scene', 'shape-rendering': 'crispEdges', 'aria-hidden': 'true' }, ...shapes);
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
    svg('circle', { cx, cy, r, fill: rgb(C.portrait), stroke: '#fff', 'stroke-width': 3 }),
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

// The glassy cube (theme.rs `paint_logo`), the same as in index.html.
function logo() {
  return svg('svg', { viewBox: '0 0 28 30', 'aria-hidden': 'true' },
    svg('polygon', { points: '14,1 27,8 14,15 1,8', fill: 'rgb(150,224,252)' }),
    svg('polygon', { points: '1,8 14,15 14,29 1,22', fill: 'rgb(40,160,228)' }),
    svg('polygon', { points: '27,8 14,15 14,29 27,22', fill: 'rgb(22,104,190)' }),
    svg('polygon', { points: '14,1 20.5,4.5 14,8 7.5,4.5', fill: 'rgb(120,214,90)' }),
    svg('polygon', { points: '1,8 14,1 18.5,3.5 7.5,11.5', fill: 'rgba(255,255,255,0.3)' }),
    svg('polygon', { points: '14,1 27,8 27,22 14,29 1,22 1,8', fill: 'none', stroke: 'rgba(255,255,255,0.8)', 'stroke-width': 1 }));
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
    case 'search': case 'friends': case 'avatar': case 'sign-in': case 'create': case 'download': case 'password': return { name: parts[0] };
    default: return { name: 'home' };
  }
}

function tabs() {
  const tab = (name, glyph, label, href) =>
    el('a', { class: `tab${route.name === name ? ' active' : ''}`, href }, icon(glyph), label);
  document.getElementById('tabs').replaceChildren(
    tab('friends', ICON.users, 'Friends', '#/friends'),
    tab('avatar', ICON.shirt, 'Avatar', '#/avatar'),
    tab('download', ICON.download, 'Download', '#/download'),
  );
  // The mark is Home, as in the app.
  document.getElementById('brand')?.classList.toggle('active', route.name === 'home');
  const who = document.getElementById('who');
  // Search sits at the right, by who you play as, as in the app.
  const search = el('a', { class: `tab iconbtn${route.name === 'search' ? ' active' : ''}`, href: '#/search', title: 'Search for players and games' }, icon(ICON.search));
  if (session) {
    who.replaceChildren(
      search,
      el('span', { class: 'caps' }, 'Playing as'),
      el('a', { class: 'name', href: `#/user/${data.me?.number ?? session.playerId}` }, session.name || 'You'),
      el('button', { class: 'chip iconbtn', title: 'Sign out', onclick: signOut }, icon(ICON.signOut)),
    );
  } else {
    who.replaceChildren(
      search,
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
  const views = { home, game, search: searchPage, friends: friendsPage, avatar, user, 'sign-in': signInPage, create: createPage, download, password: passwordPage };
  // An account that signed in with an easy password sees nothing else until it sets a new one.
  const view = session?.mustChange ? passwordPage : views[route.name] || home;
  const nodes = [];
  if (flash) {
    nodes.push(status(flash.text, flash.kind || ''));
    flash = null;
  }
  if (data.error && ['home', 'game', 'friends'].includes(route.name)) nodes.push(status(data.error, 'error'));
  loadSecurity();
  const nudge = emailNudge();
  if (nudge) nodes.push(nudge);
  page.replaceChildren(...nodes, ...[view()].flat(Infinity).filter(Boolean));
  document.title = `${titleFor(route)} · Libroxel`;
}

function titleFor(r) {
  if (r.name === 'game') return entries().find((e) => e.key === r.key)?.title || 'Game';
  return { home: 'Home', search: 'Search', friends: 'Friends', avatar: 'Avatar', user: 'Player', 'sign-in': 'Sign in', create: 'Make an account', download: 'Download', password: 'Password' }[r.name] || 'Libroxel';
}

function whereabouts(friend) {
  if (friend.online && friend.game) return [friend.game, 'var(--link)'];
  if (friend.online) return ['Online', 'var(--green)'];
  const when = friend.last_online_secs_ago;
  return [when === null || when === undefined ? 'Offline' : `Offline · ${ago(when)}`, 'var(--muted)'];
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
        const dot = f.online ? (f.game ? 'var(--link)' : 'var(--green)') : null;
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
      gamePicture(188, 150, entry.key),
      el('span', { class: 'badge', style: `background:${rgb(pictureColour(entry.key))}` }, icon(entry.symbol)),
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
    return [back, data.loaded ? el('div', { class: 'empty' }, "That game isn't here (any more).") : el('p', { class: 'soft' }, 'Loading…')];
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
  const by = entry.byId ? el('a', { href: `#/user/${entry.byId}` }, `By ${entry.by}`) : el('div', { style: 'color:var(--link)' }, `By ${entry.by}`);
  const out = [
    back,
    el('div', { class: 'hero' },
      el('div', { class: 'pic' }, gamePicture(440, 248, entry.key)),
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
    el('div', { class: 'mini' }, gamePicture(52, 52, entry.key)),
    el('div', { class: 'what' },
      el('b', {}, server.name),
      el('small', {},
        el('span', { class: `kind-${kind}` }, `${kind.charAt(0).toUpperCase()}${kind.slice(1)} server`),
        host,
        inside.map((f) => el('span', { style: 'color:var(--link);display:inline-flex;gap:4px;align-items:center' }, portrait(f.look, f.name, 16), `${f.name} is here`)))),
    el('div', { class: `fill${isFull ? ' full' : ''}` },
      server.max_players ? `${server.players} / ${server.max_players} players` : `${server.players} playing`,
      el('div', { class: 'bar' }, el('i', { style: `width:${server.max_players ? filled * 100 : 0}%` }))),
    el('button', { class: 'btn go small', disabled: isFull, onclick: () => joinServer(server) }, icon(ICON.signIn), isFull ? 'Full' : 'Join'));
}

// --- Friends ----------------------------------------------------------------

let search = { text: '', results: null, busy: false };

/** A box to find players by name, and what it found (you too, as you). */
function peopleSearch(placeholder, alsoGames = false) {
  const out = [];
  const box = el('input', { type: 'search', placeholder, value: search.text });
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
  out.push(el('div', { class: 'search' }, box, el('button', { class: 'chip', onclick: find }, icon(ICON.search), 'Find')));
  if (search.busy) out.push(el('p', { class: 'soft' }, 'Looking…'));
  if (!search.results) return out;
  const people = alsoGames ? [el('h2', { class: 'subhead' }, 'Players')] : [];
  if (!search.results.length) people.push(el('p', { class: 'soft' }, `Nobody called “${search.text}”.`));
  for (const p of search.results) people.push(personResult(p));
  if (!alsoGames) return [...out, ...people];
  const needle = search.text.toLowerCase();
  const games = entries().filter((e) => [e.title, e.by, e.about].some((t) => (t || '').toLowerCase().includes(needle)));
  return [
    ...out,
    el('div', { class: 'search-results' },
      el('div', {}, people),
      el('div', {},
        el('h2', { class: 'subhead' }, 'Games'),
        games.length ? el('div', { class: 'games' }, games.map(gameCard)) : el('p', { class: 'soft' }, `No games called “${search.text}”.`))),
  ];
}

/** One player a search found, with what you can do about them. */
function personResult(p) {
  const f = data.friends;
  const mine = session?.playerId === p.player_id;
  let action = null;
  let note = p.about || '';
  if (mine) note = "That's you";
  else if (session && f) {
    const isFriend = f.friends.some((x) => x.player_id === p.player_id);
    const asked = f.outgoing.some((x) => x.player_id === p.player_id);
    const asking = f.incoming.some((x) => x.player_id === p.player_id);
    if (isFriend) action = el('span', { class: 'soft' }, 'Friends');
    else if (asked) {
      note = 'Request sent';
      action = el('button', { class: 'chip', onclick: () => changeFriend(p.player_id, 'remove', `Took back your request to ${p.name}.`) }, 'Cancel');
    } else action = el('button', { class: 'btn go small', onclick: () => changeFriend(p.player_id, 'add', asking ? `You and ${p.name} are friends now.` : `Friend request sent to ${p.name}.`) }, icon(ICON.userPlus), asking ? 'Accept' : 'Add');
  }
  return el('div', { class: 'person' },
    portrait(null, p.name, 44),
    el('div', { class: 'what' },
      el('a', { href: `#/user/${p.number ?? p.player_id}` }, el('b', {}, p.name), el('span', { class: 'soft' }, ` #${p.number ?? ''}`)),
      el('small', { class: 'soft' }, note)),
    el('div', { class: 'buttons' }, action));
}

// --- Search ---------------------------------------------------------------

function searchPage() {
  return [
    el('h1', { class: 'title' }, 'Search'),
    el('p', { class: 'note' }, 'Find players and games.'),
    ...peopleSearch("A player's name or a game", true),
  ];
}

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
  right.push(...peopleSearch('Their name'));
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
  if (known.missing) return [back, el('div', { class: 'empty' }, 'There is nobody with that id.')];
  const p = known;
  const isMe = session?.playerId === p.player_id;
  const f = data.friends;
  let action = null;
  if (session && !isMe && f) {
    if (f.friends.some((x) => x.player_id === p.player_id)) action = el('span', { class: 'soft' }, icon(ICON.check), ' Friends');
    else if (f.outgoing.some((x) => x.player_id === p.player_id)) action = el('span', { style: 'color:var(--link)' }, 'Request sent');
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

/** Asks the Hub how your account is kept safe, and its email (only you may), once. */
function loadSecurity() {
  if (!session || (securityInfo && securityInfo.for === session.playerId)) return;
  securityInfo = { for: session.playerId, loading: true };
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel security\n${session.playerId}\n${time}`);
  api(`/accounts/${session.playerId}/security`, { query: { time, signature } })
    .then((s) => { securityInfo = { for: session.playerId, ...s }; })
    .catch((e) => { securityInfo = { for: session.playerId, error: e.message }; })
    .finally(() => render());
}

/** Whether `email` looks like an address (the Hub checks properly). */
function validEmail(email) {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim()) && email.trim().length <= 254;
}

/** A note at the top of every page while your account has no proved email. */
function emailNudge() {
  if (!session || !securityInfo || securityInfo.loading || securityInfo.error || route.name === 'avatar') return null;
  if (!securityInfo.email) return el('div', { class: 'status wait' }, 'Your account has no email yet. ', el('a', { href: '#/avatar' }, 'Add one'), ' so you can prove the account is yours.');
  if (!securityInfo.email_verified && securityInfo.sends_mail) return el('div', { class: 'status wait' }, `Prove your email: type the code we mailed to ${securityInfo.email} `, el('a', { href: '#/avatar' }, 'on your account page'), '.');
  return null;
}

let emailMessage = null;

async function sendEmailCode(address) {
  const time = Date.now();
  const email = address.trim().toLowerCase();
  const signature = signText(secretKey(), `libroxel email\n${session.playerId}\n${email}\n${time}`);
  try {
    const sent = await api('/accounts/email', { method: 'POST', body: { player_id: session.playerId, email, time, signature } });
    emailMessage = { text: sent.verified ? `${email} is already proved.` : sent.sent ? `We sent a code to ${email}. Type it below.` : `Saved ${email}. (The Hub can't send mail yet, so there's no code.)` };
    securityInfo = null;
  } catch (e) {
    emailMessage = { text: e.message, kind: 'error' };
  }
  render();
}

async function proveEmail(code) {
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel email code\n${session.playerId}\n${code.trim()}\n${time}`);
  try {
    await api('/accounts/email/verify', { method: 'POST', body: { player_id: session.playerId, code: code.trim(), time, signature } });
    emailMessage = { text: 'Your email is proved. Thanks!' };
    securityInfo = null;
  } catch (e) {
    emailMessage = { text: e.message, kind: 'error' };
  }
  render();
}

/** Your email: add one, prove it with the mailed code, or see it's done. */
function emailPanel() {
  const s = securityInfo;
  const out = [el('div', { class: 'caps', style: 'margin-top:12px' }, 'Email')];
  if (s.email && s.email_verified) out.push(el('p', {}, s.email, ' ', el('span', { style: 'color:var(--green)' }, icon(ICON.check), ' proved')));
  else if (s.email && s.sends_mail) {
    const code = el('input', { type: 'text', inputmode: 'numeric', maxlength: 6, placeholder: '6-digit code', style: 'max-width:140px' });
    out.push(el('p', { style: 'color:var(--amber)' }, `Not proved yet: type the code we mailed to ${s.email}.`));
    out.push(el('div', { class: 'search' }, code,
      el('button', { class: 'btn go small', onclick: () => proveEmail(code.value) }, 'Prove it'),
      el('button', { class: 'chip', onclick: () => sendEmailCode(s.email) }, 'Send again')));
  } else if (s.email) out.push(el('p', {}, s.email));
  else out.push(el('p', { style: 'color:var(--amber)' }, 'Your account has no email yet. Add one, so you can prove the account is yours.'));
  const box = el('input', { type: 'email', autocomplete: 'email', maxlength: 254, placeholder: s.email ? 'A new email' : 'you@example.com' });
  out.push(el('div', { class: 'search' }, box,
    el('button', { class: 'chip', onclick: () => (validEmail(box.value) ? sendEmailCode(box.value) : (emailMessage = { text: "That doesn't look like an email address.", kind: 'error' }, render())) },
      s.sends_mail ? 'Send code' : 'Save')));
  if (emailMessage) {
    out.push(status(emailMessage.text, emailMessage.kind || ''));
    emailMessage = null;
  }
  return out;
}

function security() {
  const panel = el('div', { class: 'panel', style: 'margin-top:26px' }, el('h3', {}, icon(ICON.shield), ' Your account'));
  loadSecurity();
  if (securityInfo.loading) return add(panel, [el('p', { class: 'soft' }, 'Loading…')]);
  if (securityInfo.error) return add(panel, [status(securityInfo.error, 'error')]);
  add(panel, [
    el('p', { class: 'soft' }, `You are ${session.name}. `,
      securityInfo.has_password ? 'You have a password, so you can sign in anywhere. ' : 'Set a password to sign in on other computers. ',
      el('a', { href: '#/password' }, securityInfo.has_password ? 'Change your password' : 'Set a password'), '.',
      ' ', securityInfo.two_step ? 'Two-step sign-in is on.' : 'Turn on two-step sign-in in the app, on its Avatar tab, for a code on top of your password.'),
    securityInfo.sign_ins?.length
      ? el('div', {}, el('div', { class: 'caps' }, 'Latest sign-ins'), el('ul', { class: 'signins' }, securityInfo.sign_ins.slice(0, 6).map((s) => el('li', {}, `${ago(s.secs_ago)} from ${s.from}`))))
      : null,
    ...emailPanel(),
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
/** Setting a new password; the only page while the one used to sign in is too easy. */
function passwordPage() {
  if (!session) return signInPage();
  const forced = session.mustChange;
  const password = el('input', { type: 'password', autocomplete: 'new-password' });
  const again = el('input', { type: 'password', autocomplete: 'new-password' });
  const message = el('div');
  const go = el('button', { class: 'btn go', type: 'submit' }, icon(ICON.shield), 'Set password');
  const form = el('form', { class: 'narrow' },
    el('label', { class: 'field' }, el('span', {}, `New password (at least ${MIN_PASSWORD} characters, not a common one, without your name in it)`), password),
    el('label', { class: 'field' }, el('span', {}, 'New password again'), again),
    el('div', { class: 'form-actions' }, go, !forced && el('a', { href: '#/avatar' }, 'Cancel')),
    message);
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const say = (text, kind = 'wait') => message.replaceChildren(status(text, kind));
    go.disabled = true;
    try {
      await loadCommonPasswords();
      const problem = passwordProblem(password.value, session.name || '') || (password.value !== again.value ? "The passwords don't match." : null);
      if (problem) throw new Error(problem);
      say('Locking your account with your new password… (this takes a moment, on purpose)');
      await setPassword(unhex(session.secret), session.playerId, password.value);
      const { mustChange, ...rest } = session;
      saveSession(rest, isRemembered());
      securityInfo = null;
      flash = { text: 'Password set. Use it to sign in here and in the app.' };
      goTo(forced ? '#/' : '#/avatar');
    } catch (err) {
      say(err.message, 'error');
      go.disabled = false;
    }
  });
  return [
    el('h1', { class: 'title' }, forced ? 'Pick a new password' : 'Change your password'),
    forced && status(`Your password is too easy to guess: ${forced.charAt(0).toLowerCase()}${forced.slice(1)} Set a new one to keep going.`, 'error'),
    el('p', { class: 'note' }, 'A few unrelated words work well. Your password never leaves this computer: it is turned into a key here, the same way the app does it.'),
    form,
    forced && el('p', {}, el('button', { class: 'link', type: 'button', onclick: () => { saveSession(null); data.friends = null; data.me = null; goTo('#/sign-in'); } }, 'Sign out instead')),
  ];
}

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
        await loadCommonPasswords();
        // Checked again on every sign-in: an easy password has to be changed before going on.
        const weak = passwordProblem(password.value, n);
        pending = { name: n, keys: await stretchFor(n, password.value), weak };
      } else say('Signing in…');
      const signed = await signIn(pending.name, pending.keys, needsCode ? code.value.trim() : null);
      const who = (await accountNamed(pending.name).catch(() => null))?.name || pending.name;
      const weak = pending.weak;
      saveSession({ ...signed, name: who, ...(weak ? { mustChange: weak } : {}) }, remember.checked);
      pending = null;
      flash = { text: `Signed in. Welcome back, ${who}!` };
      await refresh();
      goTo(weak ? '#/password' : '#/');
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
  const email = el('input', { type: 'email', autocomplete: 'email', maxlength: 254, placeholder: 'you@example.com' });
  const remember = el('input', { type: 'checkbox', checked: true });
  const message = el('div', {}, from && status(`There's no account called ${from.name} yet. Type the password again to make it.`, 'wait'));
  const go = el('button', { class: 'btn go', type: 'submit' }, icon(ICON.userPlus), 'Make my account');
  const form = el('form', { class: 'narrow' },
    el('label', { class: 'field' }, el('span', {}, 'Name (3 to 20 letters, digits or _, with a letter)'), name),
    el('label', { class: 'field' }, el('span', {}, `Password (at least ${MIN_PASSWORD} characters, not a common one, without your name in it)`), password),
    el('label', { class: 'field' }, el('span', {}, 'Password again'), again),
    el('label', { class: 'field' }, el('span', {}, "Email (kept private: we mail it a code to prove it's yours)"), email),
    el('label', { class: 'check' }, remember, 'Keep me signed in on this computer'),
    el('div', { class: 'form-actions' }, go),
    message);
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const say = (text, kind = 'wait') => message.replaceChildren(status(text, kind));
    const n = name.value.trim();
    try {
      await loadCommonPasswords();
    } catch (err) {
      return say(err.message, 'error');
    }
    const problem = !/^[A-Za-z0-9_]{3,20}$/.test(n) || !/[A-Za-z]/.test(n)
      ? 'A name is 3 to 20 letters, digits or underscores, with at least one letter.'
      : passwordProblem(password.value, n) || (password.value !== again.value ? "The passwords don't match." : null)
        || (validEmail(email.value) ? null : 'Type your email: the Hub mails it a code to prove it is yours.');
    if (problem) return say(problem, 'error');
    go.disabled = true;
    try {
      say('Checking the name is free…');
      const existing = await accountNamed(n);
      if (existing) throw new Error(`Someone already has the name ${existing.name} (names ignore capitals). If it's yours, sign in instead.`);
      const made = await createAccount(n, password.value, email.value.trim(), say);
      saveSession(made, remember.checked);
      securityInfo = null;
      flash = { text: `Welcome, ${n}! We mailed a code to ${email.value.trim()}: type it here to prove the email is yours.` };
      await refresh();
      goTo('#/avatar');
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
let release = null; // The newest release with every installer on it, once GitHub has said.

const INSTALLERS = ['-windows-x86_64-setup.exe', '-macos-universal.dmg', '-linux-x86_64.AppImage', '-linux-x86_64.deb'];
const RELEASE_KEY = 'libroxel-release';
const assetFor = (r, end) => (r.assets || []).find((a) => a.name.startsWith('libroxel-') && a.name.endsWith(end));

/**
 * Asks GitHub for the newest release that has the installer for every
 * platform, once. Drafts and pre-releases are left out. The last good
 * answer is kept for when GitHub can't be asked (it allows 60 questions an
 * hour from each address).
 */
function loadRelease() {
  if (release || !/^[\w.-]+\/[\w.-]+$/.test(REPO)) return;
  release = { loading: true };
  fetch(`https://api.github.com/repos/${REPO}/releases?per_page=30`, { headers: { Accept: 'application/vnd.github+json' } })
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`GitHub answered ${r.status}`))))
    .then((list) => {
      const r = (Array.isArray(list) ? list : [])
        .filter((x) => !x.draft && !x.prerelease && INSTALLERS.every((end) => assetFor(x, end)))
        .sort((x, y) => new Date(y.published_at) - new Date(x.published_at))[0];
      if (!r) throw new Error('none');
      release = { tag: r.tag_name, assets: r.assets.map((a) => ({ name: a.name, browser_download_url: a.browser_download_url })) };
      try { localStorage.setItem(RELEASE_KEY, JSON.stringify(release)); } catch { /* private window */ }
    })
    .catch((e) => {
      let saved = null;
      try { saved = JSON.parse(localStorage.getItem(RELEASE_KEY) || 'null'); } catch { /* nothing saved */ }
      release = saved?.tag && Array.isArray(saved.assets) ? saved : { error: e.message };
    })
    .finally(() => route.name === 'download' && render());
}

function download() {
  const mine = platform();
  loadRelease();
  const builds = [
    { id: 'windows', name: 'Windows', file: 'libroxel-windows-x86_64-setup.exe', ends: '-windows-x86_64-setup.exe', about: 'Windows 10 or 11. Open the setup wizard and press Next until it\'s done. Libroxel then sits in the Start menu. If SmartScreen warns you, choose More info, then Run anyway.' },
    { id: 'macos', name: 'macOS', file: 'libroxel-macos-universal.dmg', ends: '-macos-universal.dmg', about: 'macOS 11 or newer. Open it and drag Libroxel onto the Applications folder. The first time, right-click Libroxel in Applications and choose Open.' },
    { id: 'linux', name: 'Linux', file: 'libroxel-linux-x86_64.AppImage', ends: '-linux-x86_64.AppImage', about: 'Any x86-64 Linux with Vulkan. Make the file executable (right-click, Properties, Allow running as a program) and open it. It adds itself to your app menu.',
      alt: { label: '.deb for Debian and Ubuntu', file: 'libroxel-linux-x86_64.deb', ends: '-linux-x86_64.deb' } },
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
      el('h3', {}, b.name, b.id === mine ? el('span', { class: 'caps', style: 'margin-left:8px;color:var(--green)' }, 'Your computer') : null),
      el('p', {}, b.about),
      href
        ? el('a', { class: `btn small${b.id === mine ? ' go' : ''}`, href }, icon(ICON.download), 'Download')
        : el('button', { class: 'btn small', disabled: true }, icon(ICON.download), REPO && release?.loading ? 'Download' : 'Not out yet'),
      b.alt && link(b.alt) ? el('p', { class: 'soft' }, 'Or the ', el('a', { href: link(b.alt) }, b.alt.label), '.') : null);
  })));
  out.push(
    el('h2', { class: 'section' }, 'Then press Play here'),
    el('ol', { class: 'steps' },
      el('li', {}, 'Install Libroxel and open it once. That tells your computer that Play buttons on this site open Libroxel, and typing libroxel in a terminal opens it too.'),
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

