// The PlayLibre website: the app's home screen in a browser. It reads the
// same Hub as the app (games, server lists, friends and accounts), signs in
// the same way (the password is stretched here and never sent), and Play
// opens the installed app through a playlibre:// link.

import * as crypto from './vendor/crypto.js';
import { ROUTES, gamePath, href, routeOf } from './routes.js';

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

// The Hub this site talks to. `?hub=http://127.0.0.1:7430` picks another
// one for testing (remembered in this browser; `?hub=` alone forgets it).
const HUB = (() => {
  const official = document.querySelector('meta[name="playlibre-hub"]')?.content || 'https://hub.playlibre.net';
  try {
    const asked = new URLSearchParams(location.search).get('hub');
    if (asked !== null) {
      if (asked) localStorage.setItem('playlibre.hub', asked);
      else localStorage.removeItem('playlibre.hub');
    }
    return (localStorage.getItem('playlibre.hub') || official).replace(/\/+$/, '');
  } catch {
    return official;
  }
})();

const ICON = {
  house: '', games: '', users: '', play: '', broadcast: '', shirt: '',
  globe: '', left: '', right: '', signIn: '', userPlus: '',
  userMinus: '', check: '', search: '', refresh: '', bell: '',
  download: '', signOut: '', signal: '', shield: '', crown: '',
  seal: '', x: '', lock: '', user: '', hammer: '',
  like: '', dislike: '', star: '', trash: '', flag: '',
  chat: '', send: '',
  chart: '', profile: '', heart: '',
};

const BADGE_GLYPHS = {
  star: '', trophy: '', medal: '', crown: '', lightning: '', fire: '', sword: '',
  shield: '', diamond: '', flag: '', target: '', skull: '', ghost: '', rocket: '',
  heart: '', key: '', clock: '', smiley: '', flower: '', planet: '', coin: '',
  footprints: '', hammer: '', butterfly: '', confetti: '', compass: '', horse: '',
};

const PAYPAL = 'https://paypal.me/llamasjc';
const SPONSORS = 'https://github.com/sponsors/llamasjc';

// The app's colours, for drawing.
const C = {
  grass: [146, 210, 104], grassEdge: [92, 160, 64],
  portrait: [190, 226, 250],
};

// The avatar, the same file the app is built with.
const AVATAR = await fetch('/avatar.json').then((r) => r.json());
const HATS = AVATAR.hats.map((h) => h.name);
const BODIES = ['Classic', 'Brick'];

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

// Replaces what's in `node`, skipping false, null and undefined like `el`
// does (the browser's own replaceChildren would show them as text).
function fill(node, ...children) {
  node.replaceChildren();
  return add(node, children);
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
    remember(`${method} ${path} failed`);
    throw new HubError(0, 'Could not reach the Hub. Check your connection and try again.');
  }
  const text = await response.text();
  if (!response.ok) {
    remember(`${method} ${path} ${response.status}`);
    const why = text.trim() === 'this Hub is older than the site' ? 'The Hub is updating. Try again in a few minutes.' : text.trim() || response.statusText || `error ${response.status}`;
    throw new HubError(response.status, why.charAt(0).toUpperCase() + why.slice(1));
  }
  return text ? JSON.parse(text) : null;
}

// ---------------------------------------------------------------------------
// Signing in: the identity key lives in this browser once signed in
// ---------------------------------------------------------------------------

const SESSION_KEY = 'playlibre.session';
let session = loadSession();
let lastSigned = { friends: 0, party: 0, account: 0, rate: 0, message: 0, group: 0, moderate: 0, wallet: 0, badge: 0, media: 0, follow: 0, server: 0, notes: 0, place: 0, privateServer: 0, block: 0, read: 0, data: 0 };

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
  const signedIn = await api('/accounts/sign-in', { method: 'POST', body: { name, time, signature, code: code || null, device: deviceName() } });
  const sealed = unhex(signedIn.sealed);
  if (sealed.length < 12) throw new Error('The Hub sent a bad key.');
  let secret;
  try {
    secret = crypto.open(keys.seal, sealed.slice(0, 12), sealed.slice(12), utf8(signedIn.player_id));
  } catch {
    throw new Error('That password does not open this account.');
  }
  if (secret.length !== 32 || playerIdOf(secret) !== signedIn.player_id) throw new Error("The Hub sent someone else's key.");
  return { playerId: signedIn.player_id, secret: hex(secret), session: signedIn.session };
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
    const response = await fetch('/common-passwords.txt');
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
async function createAccount(name, password, email, onStep, [born, parent]) {
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
  await setBirthday(secret, playerId, born, parent);
  return { playerId, secret: hex(secret), name };
}

async function setPassword(secret, playerId, password) {
  const set = await sealPassword(secret, playerId, password, Math.max(nowSecs(), lastSigned.account + 1));
  await api('/accounts/password', { method: 'POST', body: set });
}

async function sealPassword(secret, playerId, password, time) {
  const salt = hex(random(16));
  const keys = await stretch(password, salt, PASSWORD_MEMORY_KIB, PASSWORD_PASSES);
  const nonce = random(12);
  const sealed = hex(new Uint8Array([...nonce, ...crypto.seal(keys.seal, nonce, secret, utf8(playerId))]));
  const set = {
    player_id: playerId, salt, memory_kib: PASSWORD_MEMORY_KIB, passes: PASSWORD_PASSES,
    login_id: playerIdOf(keys.login), sealed, time,
  };
  set.signature = signText(secret, `libroxel password\n${set.player_id}\n${set.salt}\n${set.memory_kib}\n${set.passes}\n${set.login_id}\n${set.sealed}\n${set.time}`);
  return set;
}

function deviceName() {
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser';
  const os = /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Windows/.test(ua) ? 'Windows' : /Mac OS/.test(ua) ? 'macOS' : /Linux/.test(ua) ? 'Linux' : '';
  return os ? `${browser} on ${os}` : browser;
}

function base32(bytes) {
  const letters = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = 0, value = 0, out = '';
  for (const b of bytes) {
    value = (value << 8) | b;
    bits += 8;
    while (bits >= 5) {
      out += letters[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) out += letters[(value << (5 - bits)) & 31];
  return out;
}

/** This browser was signed out by a reset or by signing out everywhere. */
function signedOut() {
  saveSession(null);
  securityInfo = null;
  data.friends = null;
  data.me = null;
  flash = { text: 'You were signed out.' };
  goTo('/sign-in');
}

/** Resets the password of `name` with the mailed code, moving the account to a new key. */
async function resetPassword(name, code, twoStep, password) {
  const secret = random(32);
  const playerId = playerIdOf(secret);
  const set = await sealPassword(secret, playerId, password, nowSecs());
  const done = await api('/accounts/reset/finish', {
    method: 'POST', body: { name, code: code.trim(), two_step: twoStep || null, password: set, device: deviceName() },
  });
  return { playerId: done.player_id, secret: hex(secret), session: done.session };
}

/** Signs out every other device: the account moves to a new key, sealed with the same password. */
async function signOutEverywhere(password, code) {
  const keys = await stretchFor(session.name, password);
  const secret = random(32);
  const playerId = playerIdOf(secret);
  const set = await sealPassword(secret, playerId, password, nowSecs());
  const time = Date.now();
  const text = `libroxel sign out everywhere\n${session.playerId}\n${set.signature}\n${time}`;
  const done = await api('/accounts/sign-out-everywhere', {
    method: 'POST',
    body: {
      player_id: session.playerId, password: set, code: code || null, device: deviceName(), time,
      signature: signText(secretKey(), text), login_signature: signText(keys.login, text),
    },
  });
  saveSession({ ...session, playerId: done.player_id, secret: hex(secret), session: done.session }, isRemembered());
}

// ---------------------------------------------------------------------------
// What the Hub lists, kept fresh while the page is open
// ---------------------------------------------------------------------------

const data = { official: [], games: [], servers: [], charts: { charts: [], stats: [] }, forYou: null, friends: null, party: null, me: null, loaded: false, error: null };

async function refresh() {
  const list = (path) => api(path).catch((e) => {
    if (e.status === 404) return [];
    throw e;
  });
  for (let tries = 2; tries > 0; tries--) {
    try {
      const [official, games, servers] = await Promise.all([list('/official'), list('/games'), list('/servers')]);
      data.official = official;
      data.games = games;
      data.servers = servers;
      data.error = null;
      data.charts = await api('/charts').catch(() => data.charts);
      break;
    } catch (e) {
      if (tries > 1) await new Promise((done) => setTimeout(done, 3000));
      else data.error = e.message;
    }
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
      const time = Date.now();
      const signature = signText(secretKey(), `libroxel party view\n${session.playerId}\n${time}`);
      data.party = await api(`/parties/${session.playerId}`, { query: { time, signature } });
    } catch {
      data.party = data.party || null;
    }
    try {
      const time = Date.now();
      const signature = signText(secretKey(), `libroxel for you\n${session.playerId}\n${time}`);
      data.forYou = await api(`/for-you/${session.playerId}`, { query: { time, signature } });
    } catch {
      data.forYou = data.forYou || null;
    }
    await Promise.all([loadNotifications(), loadFollowing()]);
    try {
      data.me = await api(`/accounts/${session.playerId}`);
      if (data.me.name !== session.name) saveSession({ ...session, name: data.me.name }, isRemembered());
    } catch (e) {
      if (e.status === 404) data.me = null;
      if (e.status === 410) signedOut();
    }
  } else {
    data.friends = null;
    data.party = null;
    data.forYou = null;
    data.me = null;
    data.notes = null;
    data.following = null;
  }
  data.loaded = true;
}

async function loadNotifications() {
  try {
    const time = Date.now();
    const signature = signText(secretKey(), `playlibre notifications\n${session.playerId}\n${time}`);
    data.notes = await api(`/notifications/${session.playerId}`, { query: { time, signature } });
  } catch {
    data.notes = data.notes || null;
  }
}

async function loadFollowing() {
  try {
    const time = Date.now();
    const signature = signText(secretKey(), `playlibre follows\n${session.playerId}\n${time}`);
    data.following = await api(`/follows/${session.playerId}`, { query: { time, signature } });
  } catch {
    data.following = data.following || null;
  }
}

const isFollowing = (kind, target) => (kind === 'game' ? data.following?.games : data.following?.creators)?.includes(target) || false;

async function changeFollow(kind, target, follow, name) {
  if (!session) { goTo('/sign-in'); return; }
  const time = Math.max(Date.now(), lastSigned.follow + 1);
  lastSigned.follow = time;
  const signature = signText(secretKey(), `playlibre follow\n${session.playerId}\n${kind}\n${target}\n${follow ? 1 : 0}\n${time}`);
  try {
    data.following = await api('/follows', { method: 'POST', body: { player_id: session.playerId, kind, target, follow, time, signature } });
    flash = { text: follow ? `You'll hear when ${name} ${kind === 'game' ? 'is updated' : 'puts out a new game'}.` : `You stopped following ${name}.` };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

function followButton(kind, target, name) {
  const on = isFollowing(kind, target);
  return el('button', { class: `chip${on ? ' on' : ''}`, title: on ? 'Stop following' : kind === 'game' ? 'Hear when it is updated' : 'Hear about their new games', onclick: () => changeFollow(kind, target, !on, name) },
    icon(ICON.bell), on ? 'Following' : 'Follow');
}

async function markNotificationsSeen() {
  const top = data.notes?.items?.[0];
  if (!top || !data.notes.unseen) return;
  const time = Math.max(Date.now(), lastSigned.notes + 1);
  lastSigned.notes = time;
  const up = top.notification_id;
  const signature = signText(secretKey(), `playlibre notifications seen\n${session.playerId}\n${up}\n${time}`);
  try {
    data.notes = await api('/notifications/seen', { method: 'POST', body: { player_id: session.playerId, up_to: up, time, signature } });
    render();
  } catch {
    // Seen next time.
  }
}

const NOTE_ICONS = { friend: 'userPlus', message: 'chat', group: 'shield', badge: 'star', sale: 'crown', update: 'refresh', new_game: 'games' };

function notificationsPage() {
  if (!session) return needSignIn('Sign in to see your notifications.');
  const notes = data.notes;
  if (notes?.unseen) setTimeout(markNotificationsSeen, 1500);
  return [
    el('h1', { class: 'title' }, 'Notifications'),
    el('p', { class: 'note' }, 'Friend requests, messages, invites, badges, sales and news from games and creators you follow.'),
    !notes ? el('p', { class: 'soft' }, 'Loading…')
      : !notes.items.length ? el('div', { class: 'empty' }, 'Nothing yet. Follow games and creators to hear about their updates.')
      : el('div', { class: 'notes' }, notes.items.map((n) => el('a', { class: `person note-row${n.seen ? '' : ' unseen'}`, href: n.link || '/' },
        el('div', { class: 'note-icon' }, icon(ICON[NOTE_ICONS[n.kind]] || ICON.bell)),
        el('div', { class: 'what' }, el('b', {}, n.text), el('small', { class: 'soft' }, ago(n.secs_ago)))))),
  ];
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
  const list = data.official.map((g) => ({
    key: `starter:${g.name}`, number: g.number, title: g.name, by: 'PlayLibre', about: g.about, symbol: g.symbol || ICON.games,
    source: 'starter', servers: [],
  }));
  for (const game of data.games) {
    list.push({
      key: game.game_id, number: game.number, title: game.title, by: game.author, byId: game.author_id, about: game.description,
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
 * A game's picture from its id: a cluster of cubes on a 4×4 patch of
 * grass, mirrored along the diagonal so it looks the same on the left and
 * the right. The app makes the same one (`blocks` in
 * crates/playlibre/src/menu.rs).
 */
function blocks(id) {
  let h = fnv(id);
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b) >>> 0;
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35) >>> 0;
  h = (h ^ (h >>> 16)) >>> 0;
  const heights = new Array(16).fill(0);
  let bit = 0;
  for (let y = 0; y < 4; y++) {
    for (let x = y; x < 4; x++) {
      const v = (h >>> bit) & 3;
      bit += 2;
      const inner = x >= 1 && x <= 2 && y >= 1 && y <= 2;
      const high = inner ? [1, 2, 2, 3][v] : [0, 0, 1, 2][v];
      heights[y * 4 + x] = high;
      heights[x * 4 + y] = high;
    }
  }
  return { heights, colour: hsl((h >>> 20) % 360, 0.6, 0.55) };
}

function hsl(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const [r, g, b] = [[c, x, 0], [x, c, 0], [0, c, x], [0, x, c], [x, 0, c], [c, 0, x]][Math.min(Math.floor(h / 60), 5)];
  const m = l - c / 2;
  return [r, g, b].map((v) => Math.round((v + m) * 255));
}

const pictureColour = (id) => blocks(id).colour;

/** A game's picture: its grass and cubes, as big as fits in the middle. */
function gamePicture(width, height, id) {
  const { heights, colour } = blocks(id);
  const unit = Math.min(width / 8, height / 7.5);
  const [dx, dy] = [unit * 0.866, unit * 0.5];
  const [bx, by] = [width / 2, height / 2 + unit * 0.6];
  const at = (x, y, z) => `${(bx + (x - y) * dx).toFixed(2)},${(by + (x + y - 4) * dy - z * unit).toFixed(2)}`;
  const poly = (points, fill, extra = {}) => svg('polygon', { points: points.join(' '), fill: rgb(fill), ...extra });
  const [lo, hi] = [-0.25, 4.25];
  const shapes = [poly([at(lo, lo, 0), at(hi, lo, 0), at(hi, hi, 0), at(lo, hi, 0)], C.grass, { stroke: rgb(C.grassEdge), 'stroke-width': 1 })];
  const [left, right] = [shade(colour, 0.78), shade(colour, 0.55)];
  for (let sum = 0; sum < 7; sum++) {
    for (let y = 0; y < 4; y++) {
      const x = sum - y;
      if (x < 0 || x > 3) continue;
      for (let z = 0; z < heights[y * 4 + x]; z++) {
        shapes.push(
          poly([at(x, y + 1, z + 1), at(x + 1, y + 1, z + 1), at(x + 1, y + 1, z), at(x, y + 1, z)], left),
          poly([at(x + 1, y, z + 1), at(x + 1, y + 1, z + 1), at(x + 1, y + 1, z), at(x + 1, y, z)], right),
          poly([at(x, y, z + 1), at(x + 1, y, z + 1), at(x + 1, y + 1, z + 1), at(x, y + 1, z + 1)], colour));
      }
    }
  }
  return svg('svg', { viewBox: `0 0 ${width} ${height}`, class: 'scene', 'aria-hidden': 'true' }, ...shapes);
}

/** A game's own icon when its creator put one up, or else its drawn picture. */
function gameIcon(width, height, id, info) {
  const icon = info?.media?.find((m) => m.slot === 0);
  if (!icon) return gamePicture(width, height, id);
  return el('img', { class: 'scene shot', src: mediaUrl(id, icon), alt: '', loading: 'lazy', width, height });
}

const mediaUrl = (id, m) => `${HUB}/games/${encodeURIComponent(id)}/media/${m.slot}-${m.version}`;

/** The usual look for player number `n` (`Look::for_player`). */
const lookFor = (n) => ({
  hat: 'No hat', hat_color: AVATAR.hat_color, pants: AVATAR.pants,
  shirt: AVATAR.shirts[Number(n % BigInt(AVATAR.shirts.length))], skin: AVATAR.skin,
});
const usualLook = (name) => lookFor(seed(name));

/** A catalog hat, if it fits (`CustomHat::from_json`). */
function readItem(item) {
  const v = (a, low) => Array.isArray(a) && a.length === 3 && a.every((n) => typeof n === 'number' && Number.isFinite(n) && n >= low && n <= 6) ? a : null;
  const parts = item?.parts;
  if (!Array.isArray(parts) || !parts.length || parts.length > 32) return null;
  const out = [];
  for (const p of parts) {
    const c = p?.color;
    if (!Array.isArray(c) || c.length !== 3 || !c.every((n) => Number.isInteger(n) && n >= 0 && n <= 255)) return null;
    const size = v(p.size, 0.05);
    const at = v(p.at, -6);
    if (!size || !at) return null;
    const turn = Array.isArray(p.turn) && p.turn.length === 3 && p.turn.every((n) => typeof n === 'number') ? p.turn : [0, 0, 0];
    out.push({ size, at, turn, shape: ['Ball', 'Cylinder', 'Wedge'].includes(p.shape) ? p.shape : 'Block', color: c });
  }
  return { name: typeof item.name === 'string' ? [...item.name].slice(0, 40).join('') : 'Hat', parts: out, json: item };
}

/** A look from the Hub, with anything missing taken from `usual` (`Look::from_json`). */
function readLook(look, name, usual = usualLook(name)) {
  const colour = (key) => {
    const v = look?.[key];
    return Array.isArray(v) && v.length === 3 && v.every((n) => Number.isInteger(n) && n >= 0 && n <= 255) ? v : usual[key];
  };
  const hat = typeof look?.hat === 'string' ? (HATS.includes(look.hat) ? look.hat : 'No hat') : usual.hat;
  const body = typeof look?.body === 'string' ? (BODIES.includes(look.body) ? look.body : 'Classic') : usual.body || 'Classic';
  return {
    body,
    skin: colour('skin'), shirt: colour('shirt'), pants: colour('pants'), hat_color: colour('hat_color'),
    hat,
    item: look?.item === undefined ? usual.item : readItem(look.item) || undefined,
    shirt_item: readClothing(look?.shirt_item) || undefined,
    pants_item: readClothing(look?.pants_item) || undefined,
  };
}

/** A look to save, as `Look::to_json` makes it. */
function lookJson(l) {
  const out = { skin: l.skin, shirt: l.shirt, pants: l.pants, hat: l.hat, hat_color: l.hat_color };
  if (l.body && l.body !== 'Classic') out.body = l.body;
  if (l.item) out.item = l.item.json;
  if (l.shirt_item) out.shirt_item = l.shirt_item;
  if (l.pants_item) out.pants_item = l.pants_item;
  return out;
}

/** A catalog shirt or trousers, if it fits (`Clothing::from_json`). */
function readClothing(item) {
  const c = item?.color;
  if (!Array.isArray(c) || c.length !== 3 || !c.every((n) => Number.isInteger(n) && n >= 0 && n <= 255)) return null;
  return { name: typeof item.name === 'string' ? [...item.name].slice(0, 40).join('') : 'Clothes', color: c, sleeves: item.sleeves === true };
}

const lookTag = (look) => (look ? `?v=${fnv(JSON.stringify(look)).toString(36)}` : '');

/** A round picture of someone's avatar's head and shoulders. */
function portrait(id, size, look) {
  return el('span', { class: 'portrait', style: `width:${size}px;height:${size}px` },
    el('img', { src: `${HUB}/accounts/${encodeURIComponent(id ?? '')}/pictures/head.png${lookTag(look)}`, alt: '', loading: 'lazy', onerror: (e) => e.target.remove() }));
}

/** A picture of an avatar's whole body. */
function bodyPicture(id, look) {
  return el('img', { class: 'body-pic', src: `${HUB}/accounts/${encodeURIComponent(id)}/pictures/body.png${lookTag(look)}`, alt: '', width: 150, height: 200 });
}

/** A picture of a look that isn't saved yet. */
function figure(l, name) {
  const look = encodeURIComponent(JSON.stringify(lookJson(l)));
  return [
    el('img', { src: `${HUB}/avatar/body.png?name=${encodeURIComponent(name)}&look=${look}`, alt: '', width: 220, height: 293 }),
    el('div', { class: 'figure-name' }, l.item ? l.item.name : l.hat),
  ];
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
 * Opens the installed app at `link` (playlibre://…). Browsers say nothing
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
    fill(box, ...children);
  };
  show(
    el('div', { class: 'spinner' }),
    el('h2', {}, 'Opening PlayLibre…'),
    el('p', {}, what),
    el('div', { class: 'actions' }, el('button', { class: 'btn small', onclick: close }, 'Cancel')),
  );
  let opened = false;
  const left = () => {
    opened = true;
    show(
      el('h2', {}, 'PlayLibre is opening'),
      el('p', {}, `${what} If nothing happens, open PlayLibre yourself and try again.`),
      el('div', { class: 'actions' }, el('button', { class: 'btn small', onclick: close }, 'Close')),
    );
  };
  window.addEventListener('blur', left);
  document.addEventListener('visibilitychange', left);
  const timer = setTimeout(() => {
    if (opened) return;
    show(
      el('h2', {}, 'Did PlayLibre open?'),
      el('p', {}, "If your browser asked, choose Open PlayLibre. If nothing happened, you may not have PlayLibre yet: download it, open it once, then press Play again."),
      el('div', { class: 'actions' },
        el('a', { class: 'btn go small', href: '/download', onclick: close }, icon(ICON.download), 'Download PlayLibre'),
        el('button', { class: 'btn small', onclick: () => { close(); launch(link, what); } }, 'Try again')),
    );
  }, 2500);
  document.body.append(overlay);
  window.location.href = link;
}

const appLink = (action, value) => `playlibre://${action}/${encodeURIComponent(value)}`;
const joinServer = (server) => launch(appLink('join', server.code), `Joining ${server.name}.`);

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

const page = document.getElementById('page');
let route = { name: 'home' };
let flash = null;

const parseRoute = () => routeOf(location.pathname);

function tabs() {
  const tab = (name, glyph) =>
    el('a', { class: `tab${route.name === name ? ' active' : ''}`, href: href(name) }, icon(glyph), ROUTES[name].title);
  document.getElementById('tabs').replaceChildren(
    tab('charts', ICON.chart),
    tab('groups', ICON.shield),
    tab('avatar', ICON.shirt),
    tab('creator', ICON.hammer),
    tab('market', ICON.games),
    tab('profile', ICON.profile),
    tab('download', ICON.download),
  );
  // The mark is Home, as in the app.
  document.getElementById('brand')?.classList.toggle('active', route.name === 'home');
  const who = document.getElementById('who');
  // Search sits at the right, by who you play as, as in the app.
  const search = el('a', { class: `tab iconbtn${route.name === 'search' ? ' active' : ''}`, href: '/search', title: 'Search for players and games' }, icon(ICON.search));
  const donate = el('a', { class: 'btn small donate-btn', href: '/donate', title: 'PlayLibre is open source. Donations are highly appreciated.' }, icon(ICON.heart), 'Donate');
  if (session) {
    const unread = chats.list?.reduce((n, c) => n + c.unread, 0) || 0;
    const messages = el('a', { class: `tab iconbtn${route.name === 'messages' ? ' active' : ''}`, href: '/messages', title: 'Messages' },
      icon(ICON.chat), unread > 0 && el('span', { class: 'badge' }, unread > 99 ? '99+' : unread));
    const unseen = data.notes?.unseen || 0;
    const bell = el('a', { class: `tab iconbtn${route.name === 'notifications' ? ' active' : ''}`, href: '/notifications', title: 'Notifications' },
      icon(ICON.bell), unseen > 0 && el('span', { class: 'badge' }, unseen > 99 ? '99+' : unseen));
    fill(who,
      donate,
      search,
      bell,
      data.me?.admin && el('a', { class: `tab iconbtn${route.name === 'moderation' ? ' active' : ''}`, href: '/moderation', title: 'Moderation' }, icon(ICON.flag)),
      data.me?.admin && el('a', { class: `tab iconbtn${route.name === 'stats' ? ' active' : ''}`, href: '/stats', title: 'Stats' }, icon(ICON.signal)),
      messages,
      creditsChip(),
      el('span', { class: 'caps' }, 'Playing as'),
      el('a', { class: 'name', href: '/profile' }, session.name || 'You'),
      el('button', { class: 'chip iconbtn', title: 'Sign out', onclick: signOut }, icon(ICON.signOut)),
    );
  } else {
    who.replaceChildren(
      donate,
      search,
      el('a', { class: 'chip', href: '/create' }, 'Make an account'),
      el('a', { class: 'btn go small', href: '/sign-in' }, icon(ICON.signIn), 'Sign in'),
    );
  }
}

function signOut() {
  saveSession(null);
  data.friends = null;
  data.forYou = null;
  data.me = null;
  chats.list = null;
  chats.talk = [];
  flash = { text: 'Signed out. This browser no longer has your key.' };
  goTo('/');
}

function goTo(path) {
  if (location.pathname + location.search !== path) {
    history.pushState(null, '', path);
    window.scrollTo(0, 0);
  }
  render();
}

function status(text, kind = '') {
  return el('div', { class: `status ${kind}` }, text, kind === 'error' && reportLink(text));
}

const ISSUES = 'https://github.com/llamasjc/playlibre/issues/new';
const recent = [];

function remember(line) {
  recent.push(`${new Date().toISOString().slice(11, 19)} ${line}`);
  if (recent.length > 20) recent.shift();
}

function scrub(text) {
  return String(text)
    .split('\n')
    .map((line) => line
      .replace(/((?:password|passwd|token|secret|authorization|bearer|api_?key)[^:=\n]*[:=]).*/i, '$1 [hidden]')
      .replace(/[^\s"'()<>\[\],;`]+@[^\s"'()<>\[\],;`]+\.[A-Za-z]{2,}/g, '[email]')
      .replace(/\b[0-9a-fA-F]{32,}\b/g, '[hidden]')
      .replace(/[A-Za-z0-9_\-+/=]{24,}/g, (w) => (/\d/.test(w) && /[a-z]/.test(w) && /[A-Z]/.test(w) ? '[hidden]' : w)))
    .join('\n');
}

function reportUrl(error) {
  let message = scrub(error).trim();
  const first = message.split('\n')[0];
  const title = first.length > 80 ? `${first.slice(0, 77)}...` : first;
  const log = scrub(recent.join('\n')).split('\n').filter(Boolean);
  for (;;) {
    let body = `**Error**\n\`\`\`\n${message}\n\`\`\`\n**Version** website\n**OS** ${navigator.userAgent}\n`;
    if (log.length) body += `\n**Log**\n\`\`\`\n${log.join('\n')}\n\`\`\`\n`;
    const url = `${ISSUES}?${new URLSearchParams({ title, body })}`;
    if (url.length <= 7500) return url;
    if (log.length) log.shift();
    else message = message.slice(0, Math.floor(message.length * 0.75));
  }
}

function reportLink(error) {
  return el('a', { class: 'report', href: reportUrl(error), target: '_blank', rel: 'noopener' }, 'Report');
}

function crashed(error) {
  const text = String(error?.stack || error?.message || error || 'Error');
  remember(text.split('\n')[0]);
  document.getElementById('crash')?.remove();
  const close = el('button', { class: 'link', 'aria-label': 'Close' }, icon(ICON.x));
  const bar = el('div', { id: 'crash', class: 'crash' }, status(text.split('\n')[0], 'error'), close);
  bar.querySelector('.report').href = reportUrl(text);
  close.addEventListener('click', () => bar.remove());
  document.body.append(bar);
}

window.addEventListener('error', (e) => crashed(e.error || e.message));
window.addEventListener('unhandledrejection', (e) => crashed(e.reason));

function render() {
  route = parseRoute();
  tabs();
  const views = { home, game, games: game, charts: chartsPage, profile, creator: creatorPage, market: marketPage, item: itemPage, search: searchPage, notifications: notificationsPage, friends: friendsPage, groups: groupsPage, group: groupPage, messages: messagesPage, moderation: moderationPage, stats: statsPage, avatar, user, 'sign-in': signInPage, create: createPage, download, password: passwordPage, privacy, donate: donatePage, credits: creditsPage };
  // An account that signed in with an easy password sees nothing else until it sets a new one.
  loadFamily();
  const needsBirthday = session && familyInfo && !familyInfo.loading && !familyInfo.error && !familyInfo.has_birthday;
  const view = session?.mustChange ? passwordPage : needsBirthday ? birthdayPage : views[route.name] || home;
  const nodes = [];
  if (flash) {
    nodes.push(status(flash.text, flash.kind || ''));
    flash = null;
  }
  if (data.error && ['home', 'game', 'games', 'charts', 'friends'].includes(route.name)) nodes.push(status(data.error, 'error'));
  loadSecurity();
  const nudge = emailNudge();
  if (nudge) nodes.push(nudge);
  if (session && securityInfo?.warnings?.length) nodes.push(warningsBar());
  const banned = session && bannedBar();
  if (banned) nodes.push(banned);
  page.replaceChildren(...nodes, ...[view()].flat(Infinity).filter(Boolean));
  document.title = route.name === 'home' ? 'PlayLibre: play games people make' : `${titleFor(route)} · PlayLibre`;
  const description = DESCRIPTIONS[route.name] || DESCRIPTIONS.home;
  document.querySelector('meta[name="description"]')?.setAttribute('content', description);
  document.querySelector('link[rel="canonical"]')?.setAttribute('href', location.origin + location.pathname);
}

const DESCRIPTIONS = {
  home: 'Play games people make, with your friends. Press Play and PlayLibre opens straight into the game.',
  download: 'Download PlayLibre for Windows, macOS and Linux. Free and open source.',
  privacy: 'What PlayLibre keeps about you, why, and how to have it removed.',
  donate: 'PlayLibre is free and open source. Donations are highly appreciated.',
  charts: 'The games people play most on PlayLibre, trending, top rated and new.',
  market: 'Hats, clothes, models and plugins on PlayLibre.',
  search: 'Find players and games on PlayLibre.',
  'sign-in': 'Sign in to PlayLibre.',
  create: 'Make a PlayLibre account.',
};

function titleFor(r) {
  if (isGamePage(r)) return gameFor(r)?.title || 'Game';
  if (r.name === 'group') return groupPages.get(r.id)?.group?.name || 'Group';
  return ROUTES[r.name]?.title || 'PlayLibre';
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
      portrait(session.playerId, 64, data.me?.look),
      el('div', {},
        el('h1', {}, `Hello, ${session.name || 'there'}!`),
        el('div', { class: 'soft' }, online === 1 ? '1 friend is online.' : `${online} friends are online.`))));
    const list = friends();
    out.push(el('h2', { class: 'section' }, `Friends (${list.length})`, el('a', { href: '/friends' }, 'See all ', icon(ICON.right))));
    const waiting = data.friends?.incoming?.length || 0;
    if (waiting) {
      out.push(el('a', { class: 'waiting', href: '/friends' }, icon(ICON.bell), waiting === 1 ? '1 friend request waiting' : `${waiting} friend requests waiting`));
    }
    const invite = data.party?.invites?.[0];
    if (invite) out.push(el('a', { class: 'waiting', href: '/friends' }, icon(ICON.users), `${invite.from.name} invited you to a party`));
    out.push(el('div', { class: 'friend-row' },
      el('a', { class: 'friend-tile', href: '/friends' }, el('div', { class: 'add-face' }, icon(ICON.userPlus)), el('b', {}, 'Add friends')),
      list.map((f) => {
        const [where, colour] = whereabouts(f);
        const dot = f.online ? (f.game ? 'var(--link)' : 'var(--green)') : null;
        const tile = el('a', { class: 'friend-tile', href: f.server ? '#' : `/user/${f.number ?? f.player_id}`, title: f.server ? `Join ${f.name}` : f.name },
          el('div', { class: 'face' }, portrait(f.player_id, 78, f.look), dot && el('span', { class: 'dot', style: `background:${dot}` })),
          el('b', {}, f.name),
          el('small', { style: `color:${colour}` }, where));
        if (f.server) tile.addEventListener('click', (e) => { e.preventDefault(); joinServer(f.server); });
        return tile;
      })));
  } else {
    out.push(el('div', { class: 'hello' },
      el('div', {},
        el('h1', {}, 'Play games people make'),
        el('div', { class: 'soft' }, 'Find a game, press Play, and PlayLibre opens straight into it. ',
          el('a', { href: '/sign-in' }, 'Sign in'), ' to see your friends.'))));
  }
  out.push(...picksRows());
  out.push(gamesList());
  return out;
}

function byKey(key) {
  return entries().find((e) => e.key === key);
}

/** Where a game's page is: /games/<number>/<name> once the Hub has given it
 * a number, or else by its id (/game/<id>), which moves to the numbered page
 * when it opens. */
function gameHref(entry) {
  if (entry.number !== undefined && entry.number !== null) return gamePath(entry.number, entry.title);
  return `/game/${encodeURIComponent(entry.key)}`;
}

/** A link to the page of the game with this id, from what's known of it. */
function gameHrefOf(id, number, title) {
  const known = byKey(id);
  return gameHref(known || { key: id, number, title });
}

function gameRow(title, list, link) {
  if (!list.length) return [];
  return [el('h2', { class: 'section' }, title, link && el('a', { href: link }, 'See all ', icon(ICON.right))), el('div', { class: 'games row' }, list.map(gameCard))];
}

function picksRows() {
  const out = [];
  const mine = data.forYou;
  if (mine) out.push(...gameRow('Continue', mine.continue.map((p) => byKey(p.game_id)).filter(Boolean).slice(0, 12)));
  const picks = mine?.recommended?.length ? mine.recommended.map(byKey).filter(Boolean) : chartGames('popular');
  out.push(...gameRow('Recommended', picks.slice(0, 12), '/charts'));
  return out;
}

function chartGames(id) {
  const chart = data.charts.charts.find((c) => c.id === id);
  return (chart?.game_ids || []).map(byKey).filter(Boolean);
}

function liked(stats) {
  return stats && stats.likes + stats.dislikes ? `${Math.round((100 * stats.likes) / (stats.likes + stats.dislikes))}%` : null;
}

function chartsPage() {
  const charts = data.charts.charts;
  const picked = new URLSearchParams(location.search).get('c');
  const current = charts.find((c) => c.id === picked) || charts[0];
  const out = [el('h1', { class: 'title' }, 'Charts')];
  if (!data.loaded) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  if (!current) return [...out, el('div', { class: 'empty' }, 'No games yet.')];
  out.push(el('div', { class: 'hats' }, charts.map((c) => el('a', { class: `chip${c === current ? ' on' : ''}`, href: `/charts?c=${encodeURIComponent(c.id)}` }, c.title))));
  out.push(el('div', { class: 'games' }, current.game_ids.map(byKey).filter(Boolean).map((g, i) => {
    const card = gameCard(g);
    card.querySelector('.pic').append(el('span', { class: 'rank' }, String(i + 1)));
    const rating = liked(statsFor(g.key));
    if (rating) card.append(el('div', { class: 'meta idle' }, `${rating} liked`));
    return card;
  })));
  return out;
}

function statsFor(key) {
  return data.charts.stats.find((s) => s.game_id === key);
}

function gamesList() {
  return [
    el('h2', { class: 'section' }, 'Games'),
    el('p', { class: 'note' }, 'Pick a game to see its servers, or press Play on its page to jump in.'),
    !data.loaded && el('p', { class: 'soft' }, 'Loading…'),
    el('div', { class: 'games' }, entries().map(gameCard)),
  ];
}

function gameCard(entry) {
  const inside = friends().filter((f) => entry.servers.some((s) => s.server_id === f.server?.server_id));
  let meta;
  if (inside.length) meta = el('div', { class: 'meta' }, icon(ICON.users), ` ${count(entry.playing)} playing · ${inside.map((f) => f.name).join(', ')}`);
  else if (entry.playing) meta = el('div', { class: 'meta live' }, icon(ICON.users), ` ${count(entry.playing)} playing`);
  else meta = el('div', { class: 'meta idle' }, `By ${entry.by}`);
  return el('a', { class: 'game', href: gameHref(entry) },
    el('div', { class: 'pic' },
      gameIcon(188, 150, entry.key, entry.game),
      el('span', { class: 'badge', style: `background:${rgb(pictureColour(entry.key))}` }, icon(entry.symbol)),
      inside.length ? el('span', { class: 'faces' }, inside.slice(0, 3).map((f) => portrait(f.player_id, 26, f.look))) : null),
    el('h3', {}, entry.title),
    meta);
}

// --- A game's page ----------------------------------------------------------

const view = { hideFull: false };

const isGamePage = (r) => r.name === 'game' || r.name === 'games';

// Games the lists don't hold (private, or past the first lot), by what the
// address asked for: undefined while asking, null when the Hub has none.
const lookedUp = new Map();

/** The game a page's address is for: /games/<number>/… or /game/<id>. */
function gameFor(r) {
  const asked = r.name === 'games' ? r.number : r.key;
  if (!asked) return null;
  const listed = entries().find((e) => (r.name === 'games' ? String(e.number) === asked : e.key === asked));
  if (listed) return listed;
  if (!data.loaded) return undefined;
  if (asked.startsWith('starter:') || asked.startsWith('live:')) return null;
  if (!lookedUp.has(asked)) {
    lookedUp.set(asked, undefined);
    api(`/games/${encodeURIComponent(asked)}`)
      .then((g) => lookedUp.set(asked, {
        key: g.game_id, number: g.number, title: g.title, by: g.author, byId: g.author_id, about: g.description,
        symbol: ICON.globe, source: 'published', game: g, servers: [], playing: 0,
      }))
      .catch(() => lookedUp.set(asked, null))
      .then(() => { if (isGamePage(route)) render(); });
  }
  return lookedUp.get(asked);
}

function game() {
  const entry = gameFor(route);
  const back = el('a', { class: 'back', href: '/' }, icon(ICON.left), 'Home');
  if (!entry) {
    return [back, entry === undefined ? el('p', { class: 'soft' }, 'Loading…') : el('div', { class: 'empty' }, "That game isn't here (any more).")];
  }
  // One address per game: a bare number, a wrong or old name, or an old
  // link by id all move to /games/<number>/<name>.
  const canonical = gameHref(entry);
  if (canonical.startsWith('/games/') && location.pathname !== canonical) {
    history.replaceState(null, '', canonical + location.search + location.hash);
  }
  const best = bestServer(entry.servers);
  const hostable = entry.source !== 'live';
  const stats = [[count(entry.playing), 'Playing'], [String(entry.servers.length), 'Servers']];
  if (entry.game) stats.push([count(entry.game.plays), 'Visits'], [`v${entry.game.version}`, 'Version']);
  const rating = liked(statsFor(entry.key));
  if (rating) stats.push([rating, 'Liked']);
  const hint = best ? `Joins ${best.name}` : hostable ? 'No servers yet: starts one that friends can join' : 'Nobody is hosting it right now';
  const play = el('button', { class: 'btn go big', disabled: !best && !hostable }, icon(ICON.play), 'Play');
  play.addEventListener('click', () => {
    if (best) joinServer(best);
    else launch(appLink('play', entry.key), `Starting ${entry.title}.`);
  });
  const group = entry.game?.group;
  const by = group
    ? el('a', { href: `/group/${group.group_id}` }, `By ${group.name} `, badge(group))
    : entry.byId ? el('a', { href: `/user/${entry.byId}` }, `By ${entry.by}`) : el('div', { style: 'color:var(--link)' }, `By ${entry.by}`);
  const out = [
    back,
    el('div', { class: 'hero' },
      el('div', { class: 'pic' }, gameIcon(440, 248, entry.key, entry.game)),
      el('div', {},
        el('h1', {}, entry.title),
        by,
        el('div', { class: 'stats' }, stats.map(([v, l]) => el('div', {}, el('b', {}, v), el('span', { class: 'caps' }, l)))),
        el('div', { class: 'actions' },
          play,
          hostable && el('button', { class: 'btn big', title: 'Host your own server of this game; it shows on this list', onclick: () => launch(appLink('host', entry.key), `Starting a new server of ${entry.title}.`) }, icon(ICON.broadcast), 'New server'),
          entry.game && followButton('game', entry.key, entry.title)),
        el('div', { class: 'hint' }, hint))),
  ];
  const shots = (entry.game?.media || []).filter((m) => m.slot > 0);
  if (shots.length) {
    out.push(el('div', { class: 'shots' }, shots.map((m) =>
      el('a', { href: mediaUrl(entry.key, m), target: '_blank', rel: 'noopener' },
        el('img', { src: mediaUrl(entry.key, m), alt: `A screenshot of ${entry.title}`, loading: 'lazy' })))));
  }
  if (entry.about) out.push(el('div', { class: 'about' }, el('div', { class: 'caps' }, 'About'), entry.about));
  const genre = entry.game?.genre;
  const tags = entry.game?.tags || [];
  if (genre || tags.length) {
    out.push(el('div', { class: 'tags' },
      genre && el('a', { class: 'chip', href: `/search?genre=${encodeURIComponent(genre)}` }, genre),
      tags.map((t) => el('a', { class: 'chip soft-chip', href: `/search?tag=${encodeURIComponent(t)}` }, `#${t}`))));
  }
  if (hostable) out.push(ratingBar(entry), reportPanel('game', entry.key));
  if (entry.game) out.push(passesPanel(entry), privateServersPanel(entry));

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
  if (hostable) out.push(gameBadges(entry.key), commentsPanel(entry));
  return out;
}

// --- Likes, favorites and comments --------------------------------------------

let rated = { key: null, who: null, ratings: null, comments: null, text: '', busy: false };

function loadRatings(key) {
  rated = { key, who: session?.playerId, ratings: null, comments: null, text: '', busy: false };
  const query = session ? { player_id: session.playerId } : undefined;
  Promise.all([api(`/games/${encodeURIComponent(key)}/ratings`, { query }).catch(() => null), api(`/games/${encodeURIComponent(key)}/comments`).catch(() => [])])
    .then(([ratings, comments]) => {
      if (rated.key !== key) return;
      rated.ratings = ratings;
      rated.comments = comments;
      if (isGamePage(route)) render();
    });
}

function signedTime() {
  const time = Math.max(Date.now(), lastSigned.rate + 1);
  lastSigned.rate = time;
  return time;
}

async function rate(key, vote, favorite) {
  const time = signedTime();
  const signature = signText(secretKey(), `libroxel rate\n${session.playerId}\n${key}\n${vote}\n${favorite}\n${time}`);
  try {
    rated.ratings = await api(`/games/${encodeURIComponent(key)}/ratings`, { method: 'POST', body: { player_id: session.playerId, vote, favorite, time, signature } });
    favorites.delete(session.playerId);
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

function ratingBar(entry) {
  if (rated.key !== entry.key || rated.who !== session?.playerId) loadRatings(entry.key);
  const r = rated.ratings || { likes: 0, dislikes: 0, favorites: 0, vote: 'none', favorite: false };
  const total = r.likes + r.dislikes;
  const can = !!session;
  const tip = can ? undefined : 'Sign in to rate games';
  const press = (vote, favorite) => can && rate(entry.key, vote, favorite);
  return el('div', { class: 'rating' },
    el('button', { class: `chip${r.vote === 'like' ? ' on' : ''}`, disabled: !can, title: tip, onclick: () => press(r.vote === 'like' ? 'none' : 'like', r.favorite) }, icon(ICON.like), count(r.likes)),
    el('button', { class: `chip${r.vote === 'dislike' ? ' on' : ''}`, disabled: !can, title: tip, onclick: () => press(r.vote === 'dislike' ? 'none' : 'dislike', r.favorite) }, icon(ICON.dislike), count(r.dislikes)),
    total > 0 && el('div', { class: 'ratio', title: `${r.likes} of ${total} liked it` }, el('i', { style: `width:${(r.likes / total) * 100}%` })),
    el('button', { class: `chip${r.favorite ? ' on' : ''}`, disabled: !can, title: tip, onclick: () => press(r.vote, !r.favorite) }, icon(ICON.star), r.favorite ? 'Favorited' : 'Favorite', ` ${count(r.favorites)}`),
    can && entry.game && entry.byId !== session.playerId && el('button', { class: 'chip', title: 'Report', onclick: () => startReport('game', entry.key, entry.title) }, icon(ICON.flag)));
}

async function postComment(key) {
  const text = rated.text.trim();
  if (!text) return;
  const time = signedTime();
  const signature = signText(secretKey(), `libroxel comment\n${session.playerId}\n${key}\n${rated.text}\n${time}`);
  rated.busy = true;
  render();
  try {
    const said = await api(`/games/${encodeURIComponent(key)}/comments`, { method: 'POST', body: { player_id: session.playerId, text: rated.text, time, signature } });
    rated.comments = [said, ...(rated.comments || [])];
    rated.text = '';
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  rated.busy = false;
  render();
}

async function removeComment(c) {
  const time = signedTime();
  const signature = signText(secretKey(), `libroxel uncomment\n${session.playerId}\n${c.comment_id}\n${time}`);
  try {
    await api(`/comments/${c.comment_id}`, { method: 'DELETE', body: { player_id: session.playerId, time, signature } });
    rated.comments = rated.comments.filter((x) => x.comment_id !== c.comment_id);
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

const REPORT_REASONS = ['Rude, hateful or bullying', 'Sexual or not OK for kids', 'A scam or a cheat', "Something else that's wrong"];
let reporting = null;

function startReport(kind, target, name) {
  reporting = { kind, target, name, reason: REPORT_REASONS[0], note: '', busy: false };
  render();
}

async function sendReport() {
  const r = reporting;
  const time = nowSecs();
  const evidence = r.note.trim();
  const tail = evidence ? `\n${evidence}` : '';
  const signature = signText(secretKey(), `libroxel report\n${session.playerId}\n${r.kind}\n${r.target}\n${r.reason}\n${time}${tail}`);
  r.busy = true;
  render();
  try {
    await api('/reports', { method: 'POST', body: { reporter_id: session.playerId, kind: r.kind, target: r.target, reason: r.reason, evidence, time, signature } });
    flash = { text: 'Reported. Thanks.' };
    reporting = null;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
    r.busy = false;
  }
  render();
}

function reportPanel(kind, target) {
  const r = reporting;
  if (!r || r.kind !== kind || r.target !== target) return null;
  const reason = el('select', {}, REPORT_REASONS.map((x) => el('option', { value: x, selected: x === r.reason }, x)));
  reason.addEventListener('change', () => { r.reason = reason.value; });
  const note = el('textarea', { rows: 3, maxlength: 2000, placeholder: 'What happened (optional)' }, r.note);
  note.addEventListener('input', () => { r.note = note.value; });
  return el('div', { class: 'report-card' },
    el('b', {}, `Report ${r.name}`),
    reason, note,
    el('div', { class: 'buttons' },
      el('button', { class: 'btn small', disabled: r.busy, onclick: sendReport }, icon(ICON.flag), 'Send'),
      el('button', { class: 'chip', onclick: () => { reporting = null; render(); } }, 'Cancel')));
}

async function reportComment(c) {
  const time = nowSecs();
  const target = String(c.comment_id);
  const reason = 'Not OK';
  const signature = signText(secretKey(), `libroxel report\n${session.playerId}\ncomment\n${target}\n${reason}\n${time}`);
  try {
    await api('/reports', { method: 'POST', body: { reporter_id: session.playerId, kind: 'comment', target, reason, time, signature } });
    flash = { text: 'Reported. Thanks.' };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

function commentsPanel(entry) {
  const list = rated.key === entry.key ? rated.comments : null;
  const out = [el('h2', { class: 'section' }, `Comments${list ? ` (${list.length})` : ''}`)];
  if (session) {
    const box = el('textarea', { rows: 2, maxlength: 300, placeholder: 'Say something about this game' });
    box.value = rated.text;
    box.addEventListener('input', () => { rated.text = box.value; });
    box.addEventListener('keydown', (e) => e.key === 'Enter' && !e.shiftKey && (e.preventDefault(), postComment(entry.key)));
    out.push(el('div', { class: 'comment-box' }, box, el('button', { class: 'btn small', disabled: rated.busy, onclick: () => postComment(entry.key) }, 'Post')));
  } else {
    out.push(el('p', { class: 'soft' }, el('a', { href: '/sign-in' }, 'Sign in'), ' to comment.'));
  }
  if (!list) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  if (!list.length) out.push(el('p', { class: 'soft' }, 'No comments yet.'));
  const mine = (c) => session && (c.player_id === session.playerId || entry.byId === session.playerId);
  for (const c of list) {
    out.push(el('div', { class: 'comment' },
      portrait(c.player_id, 36),
      el('div', { class: 'what' },
        el('div', {}, el('a', { href: `/user/${c.number ?? c.player_id}` }, el('b', {}, c.name)), el('span', { class: 'soft' }, ` · ${ago(c.secs_ago)}`)),
        el('div', { class: 'text' }, c.text)),
      el('div', { class: 'buttons' },
        mine(c) && el('button', { class: 'chip', title: 'Delete', onclick: () => removeComment(c) }, icon(ICON.trash)),
        session && c.player_id !== session.playerId && el('button', { class: 'chip', title: 'Report', onclick: () => reportComment(c) }, icon(ICON.flag)))));
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
    el('div', { class: 'mini' }, gameIcon(52, 52, entry.key, entry.game)),
    el('div', { class: 'what' },
      el('b', {}, server.name),
      el('small', {},
        el('span', { class: `kind-${kind}` }, `${kind.charAt(0).toUpperCase()}${kind.slice(1)} server`),
        host,
        inside.map((f) => el('span', { style: 'color:var(--link);display:inline-flex;gap:4px;align-items:center' }, portrait(f.player_id, 16, f.look), `${f.name} is here`)))),
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
    portrait(p.player_id, 44),
    el('div', { class: 'what' },
      el('a', { href: `/user/${p.number ?? p.player_id}` }, el('b', {}, p.name), el('span', { class: 'soft' }, ` #${p.number ?? ''}`)),
      el('small', { class: 'soft' }, note)),
    el('div', { class: 'buttons' }, action));
}

// --- Search ---------------------------------------------------------------

// Games with one tag or genre, from the Hub: what was asked, and the answer.
let tagged = { key: null, games: null, error: null };

function searchPage() {
  const asked = new URLSearchParams(location.search);
  const tag = asked.get('tag') || '';
  const genre = asked.get('genre') || '';
  if (tag || genre) {
    const key = `${tag}\n${genre}`;
    if (tagged.key !== key) {
      tagged = { key, games: null, error: null };
      api('/games', { query: { tag, genre } })
        .then((games) => { if (tagged.key === key) tagged.games = games; })
        .catch((e) => { if (tagged.key === key) tagged.error = e.message; })
        .finally(() => { if (route.name === 'search') render(); });
    }
    const shown = (tagged.games || []).map((g) => byKey(g.game_id) || { key: g.game_id, title: g.title, by: g.author, servers: [], playing: g.playing, game: g, number: g.number, symbol: 'cube' });
    return [
      el('a', { class: 'back', href: '/search' }, icon(ICON.left), 'Search'),
      el('h1', { class: 'title' }, tag ? `#${tag}` : genre),
      el('p', { class: 'note' }, tag ? 'Games with this tag.' : `${genre} games.`),
      tagged.error ? status(tagged.error, 'error')
        : !tagged.games ? el('p', { class: 'soft' }, 'Looking…')
        : shown.length ? el('div', { class: 'games' }, shown.map(gameCard)) : el('p', { class: 'soft' }, 'No games yet.'),
    ];
  }
  return [
    el('h1', { class: 'title' }, 'Search'),
    el('p', { class: 'note' }, 'Find players and games.'),
    ...peopleSearch("A player's name or a game", true),
  ];
}

async function changeParty(action, target, done) {
  const time = Math.max(Date.now(), lastSigned.party + 1);
  lastSigned.party = time;
  const signature = signText(secretKey(), `libroxel party\n${session.playerId}\n${action}\n${target}\n${time}`);
  try {
    data.party = await api('/parties', { method: 'POST', body: { player_id: session.playerId, action, target, time, signature } });
    if (done) flash = { text: done };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

function partyRows() {
  const out = [];
  for (const invite of data.party?.invites || []) {
    out.push(el('div', { class: 'person' },
      portrait(invite.from.player_id, 44),
      el('div', { class: 'what' }, el('b', {}, invite.from.name), el('small', { style: 'color:var(--amber)' }, 'Invited you to a party')),
      el('div', { class: 'buttons' },
        el('button', { class: 'btn go small', onclick: () => changeParty('accept', invite.party_id, `You joined ${invite.from.name}'s party.`) }, icon(ICON.check), 'Accept'),
        el('button', { class: 'chip', onclick: () => changeParty('decline', invite.party_id) }, 'Decline'))));
  }
  const party = data.party?.party;
  if (!party) return out;
  const leading = party.leader_id === session.playerId;
  out.push(el('h2', { class: 'subhead' }, `Party (${party.members.length})`,
    el('button', { class: 'chip', style: 'margin-left:auto', onclick: () => changeParty('leave', '', 'You left the party.') }, 'Leave party')));
  for (const m of party.members) {
    const mine = m.player_id === session.playerId;
    const [text, colour] = m.game ? [m.game, 'var(--link)'] : m.online ? ['Online', 'var(--green)'] : ['Offline', 'var(--muted)'];
    out.push(el('div', { class: 'person' },
      portrait(m.player_id, 44, m.look),
      el('div', { class: 'what' }, el('b', {}, m.leader ? icon(ICON.crown) : null, m.leader ? ' ' : null, m.name), el('small', { style: `color:${colour}` }, text)),
      el('div', { class: 'buttons' },
        !mine && m.server && el('button', { class: 'btn go small', onclick: () => joinServer(m.server) }, icon(ICON.signIn), 'Join'),
        leading && !mine && el('button', { class: 'chip', onclick: () => changeParty('remove', m.player_id) }, 'Remove'))));
  }
  for (const p of party.invited) {
    out.push(el('div', { class: 'person' }, portrait(p.player_id, 44),
      el('div', { class: 'what' }, el('b', {}, p.name), el('small', { class: 'soft' }, 'Invited'))));
  }
  return out;
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
      el('a', { class: 'btn go', href: '/sign-in' }, icon(ICON.signIn), 'Sign in'),
      el('a', { href: '/create' }, 'or make an account')),
  ];
}

function friendsPage() {
  if (!session) return needSignIn('Sign in to see your friends, where they are playing, and join them.');
  const f = data.friends || { friends: [], incoming: [], outgoing: [] };
  const online = f.friends.filter((x) => x.online).length;
  const party = data.party?.party;
  const inParty = new Set([...(party?.members || []), ...(party?.invited || [])].map((p) => p.player_id));
  const left = [...partyRows(), el('h2', { class: 'subhead' }, `Your friends (${f.friends.length}) · ${online} online`)];
  if (!f.friends.length) left.push(el('div', { class: 'empty' }, 'No friends yet. Find people by name on the right.'));
  for (const friend of f.friends) {
    const [where, colour] = whereabouts(friend);
    const text = friend.online && friend.game ? `Playing ${friend.game}${friend.server ? ` on ${friend.server.name}` : ''}` : where;
    left.push(el('div', { class: 'person' },
      portrait(friend.player_id, 44, friend.look),
      el('div', { class: 'what' }, el('a', { href: `/user/${friend.number ?? friend.player_id}` }, el('b', {}, friend.name)), el('small', { style: `color:${colour}` }, text)),
      el('div', { class: 'buttons' },
        friend.online && !inParty.has(friend.player_id) && el('button', { class: 'chip', onclick: () => changeParty('invite', friend.player_id, `Invited ${friend.name}.`) }, icon(ICON.users), 'Invite'),
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
        portrait(p.player_id, 44),
        el('div', { class: 'what' }, el('a', { href: `/user/${p.number ?? p.player_id}` }, el('b', {}, p.name)), el('small', { class: 'soft' }, 'Wants to be your friend')),
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
        portrait(p.player_id, 44),
        el('div', { class: 'what' }, el('b', {}, p.name), el('small', { class: 'soft' }, 'Waiting for them to accept')),
        el('div', { class: 'buttons' }, el('button', { class: 'chip', onclick: () => changeFriend(p.player_id, 'remove', `Took back your request to ${p.name}.`) }, 'Cancel'))));
    }
  }
  return [el('h1', { class: 'title' }, 'Friends'), el('div', { class: 'columns' }, el('div', {}, left), el('div', {}, right))];
}

// --- Messages ---------------------------------------------------------------

const MAX_MESSAGE = 500;
const chats = { list: null, with: null, talk: [], draft: '', busy: false, error: null, reporting: null, at: 0, blocked: [], unread: false };

function readSigned() {
  const time = Date.now();
  return { time, signature: signText(secretKey(), `libroxel messages\n${session.playerId}\n${time}`) };
}

async function loadChats() {
  if (!session) return;
  chats.at = Date.now();
  try {
    chats.list = await api(`/messages/${session.playerId}`, { query: readSigned() });
    if (route.name === 'messages') chats.blocked = await api(`/blocks/${session.playerId}`, { query: readSigned() });
    if (chats.with) chats.talk = await api(`/messages/${session.playerId}/${chats.with}`, { query: readSigned() });
    chats.error = null;
  } catch (e) {
    chats.error = e.message;
  }
}

function redrawChats() {
  if (route.name !== 'messages') {
    tabs();
    return;
  }
  const box = document.getElementById('chat-box');
  const typing = box && document.activeElement === box;
  render();
  const again = document.getElementById('chat-box');
  if (typing && again) {
    again.focus();
    again.setSelectionRange(again.value.length, again.value.length);
  }
}

async function sendChat() {
  const text = chats.draft.trim();
  if (!text || chats.busy || !chats.with) return;
  const to = chats.with;
  const time = Math.max(Date.now(), lastSigned.message + 1);
  lastSigned.message = time;
  const signature = signText(secretKey(), `libroxel message\n${session.playerId}\n${to}\n${text}\n${time}`);
  chats.busy = true;
  try {
    await api('/messages', { method: 'POST', body: { from_id: session.playerId, to_id: to, text, time, signature } });
    chats.draft = '';
    chats.error = null;
  } catch (e) {
    chats.error = e.message;
  }
  chats.busy = false;
  const error = chats.error;
  await loadChats();
  chats.error = chats.error || error;
  redrawChats();
}

async function reportChat(message) {
  const time = Date.now();
  const reason = 'message';
  const signature = signText(secretKey(), `libroxel report\n${session.playerId}\nmessage\n${message.message_id}\n${reason}\n${time}`);
  try {
    await api('/reports', { method: 'POST', body: { reporter_id: session.playerId, kind: 'message', target: message.message_id, reason, time, signature } });
    flash = { text: 'Reported.' };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  chats.reporting = null;
  redrawChats();
}

async function readAllChats() {
  const time = Math.max(Date.now(), lastSigned.read + 1);
  lastSigned.read = time;
  const signature = signText(secretKey(), `libroxel messages read\n${session.playerId}\n${time}`);
  try {
    await api('/messages/read', { method: 'POST', body: { player_id: session.playerId, time, signature } });
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  await Promise.all([loadChats(), refresh()]);
  redrawChats();
}

async function changeBlock(otherId, block, name) {
  const time = Math.max(Date.now(), lastSigned.block + 1);
  lastSigned.block = time;
  const signature = signText(secretKey(), `libroxel block\n${session.playerId}\n${otherId}\n${block ? 1 : 0}\n${time}`);
  try {
    chats.blocked = await api('/blocks', { method: 'POST', body: { player_id: session.playerId, other_id: otherId, block, time, signature } });
    flash = { text: block ? `Blocked ${name}.` : `Unblocked ${name}.` };
    if (block && chats.with === otherId) history.pushState(null, '', '/messages');
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  await Promise.all([loadChats(), refresh()]);
  render();
}

function messagesPage() {
  if (!session) return needSignIn('Sign in to message your friends.');
  if (chats.with !== (route.id || null)) {
    chats.with = route.id || null;
    chats.talk = [];
    chats.draft = '';
    chats.reporting = null;
    chats.error = null;
    loadChats().then(redrawChats);
  }
  const list = chats.list || [];
  const talked = new Set(list.map((c) => c.with.player_id));
  const unread = list.reduce((n, c) => n + c.unread, 0);
  const left = [el('div', { class: 'form-actions' },
    el('button', { class: `chip${chats.unread ? ' on' : ''}`, onclick: () => { chats.unread = !chats.unread; render(); } }, `Unread (${unread})`),
    unread > 0 && el('button', { class: 'chip', onclick: readAllChats }, icon(ICON.check), 'Mark all read'))];
  for (const c of list) {
    if (chats.unread && !c.unread && c.with.player_id !== chats.with) continue;
    const mine = c.last.from_id === session.playerId;
    left.push(el('a', { class: `person chat${c.with.player_id === chats.with ? ' on' : ''}`, href: `/messages/${c.with.player_id}` },
      portrait(c.with.player_id, 40, c.look),
      el('div', { class: 'what' },
        el('b', {}, c.with.name),
        el('small', { class: 'soft' }, mine ? `You: ${c.last.text}` : c.last.text)),
      c.unread > 0 && el('span', { class: 'badge' }, c.unread)));
  }
  for (const f of chats.unread ? [] : friends().filter((x) => !talked.has(x.player_id))) {
    left.push(el('a', { class: `person chat${f.player_id === chats.with ? ' on' : ''}`, href: `/messages/${f.player_id}` },
      portrait(f.player_id, 40, f.look),
      el('div', { class: 'what' }, el('b', {}, f.name), el('small', { class: 'soft' }, whereabouts(f)[0]))));
  }
  if (left.length === 1) left.push(el('div', { class: 'empty' }, !chats.list ? 'Loading…' : chats.unread ? 'Nothing unread.' : 'Add friends to message them.'));
  if (chats.blocked.length) {
    left.push(el('h2', { class: 'subhead' }, 'Blocked'));
    for (const b of chats.blocked) {
      left.push(el('div', { class: 'person' },
        el('div', { class: 'what' }, el('b', {}, b.name)),
        el('button', { class: 'chip', onclick: () => changeBlock(b.player_id, false, b.name) }, 'Unblock')));
    }
  }

  const right = [];
  const other = chats.with && (list.find((c) => c.with.player_id === chats.with)?.with || friends().find((f) => f.player_id === chats.with));
  if (!chats.with) right.push(el('div', { class: 'empty' }, 'Pick a friend.'));
  else {
    right.push(el('h2', { class: 'subhead' }, other?.name || '', ' ',
      el('button', { class: 'chip', onclick: () => changeBlock(chats.with, true, other?.name || '') }, 'Block')));
    const log = el('div', { class: 'chat-log', id: 'chat-log' });
    if (!chats.talk.length) log.append(el('p', { class: 'soft' }, 'No messages yet.'));
    for (const m of chats.talk) {
      const mine = m.from_id === session.playerId;
      const flag = !mine && (chats.reporting === m.message_id
        ? el('span', {}, el('button', { class: 'link', onclick: () => reportChat(m) }, 'Report'), ' ', el('button', { class: 'link', onclick: () => { chats.reporting = null; redrawChats(); } }, 'Cancel'))
        : el('button', { class: 'link flag', title: 'Report', onclick: () => { chats.reporting = m.message_id; redrawChats(); } }, icon(ICON.flag)));
      log.append(el('div', { class: `bubble${mine ? ' mine' : ''}` },
        el('div', { class: 'text' }, m.text),
        el('small', { class: 'soft' }, ago(nowSecs() - m.sent), ' ', flag)));
    }
    right.push(log);
    const box = el('input', { type: 'text', id: 'chat-box', maxlength: MAX_MESSAGE, placeholder: 'Message', value: chats.draft, autocomplete: 'off' });
    box.addEventListener('input', () => { chats.draft = box.value; });
    box.addEventListener('keydown', (e) => e.key === 'Enter' && sendChat());
    right.push(el('div', { class: 'search' }, box, el('button', { class: 'btn go small', disabled: chats.busy, onclick: sendChat }, icon(ICON.send), 'Send')));
    if (chats.error) right.push(status(chats.error, 'error'));
    requestAnimationFrame(() => document.getElementById('chat-log')?.scrollTo(0, 1e9));
  }
  return [el('h1', { class: 'title' }, 'Messages'), el('div', { class: 'columns chats' }, el('div', {}, left), el('div', {}, right))];
}

// --- A player's page --------------------------------------------------------

const profiles = new Map();

function user() {
  const id = route.id;
  const back = el('a', { class: 'back', href: '/' }, icon(ICON.left), 'Home');
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
    if (f.friends.some((x) => x.player_id === p.player_id)) action = el('span', { class: 'soft' }, icon(ICON.check), ' Friends ', el('a', { class: 'chip', href: `/messages/${p.player_id}` }, icon(ICON.chat), 'Message'));
    else if (f.outgoing.some((x) => x.player_id === p.player_id)) action = el('span', { style: 'color:var(--link)' }, 'Request sent');
    else action = el('button', { class: 'btn go small', onclick: () => changeFriend(p.player_id, 'add', `Friend request sent to ${p.name}.`).then(() => profiles.delete(id)) }, icon(ICON.userPlus), 'Add friend');
  }
  const days = Math.floor(p.joined_secs_ago / 86400);
  return [
    back,
    el('div', { class: 'hello' },
      portrait(p.player_id, 84, p.look),
      el('div', {},
        el('h1', {}, p.name, ' ', p.badge && badge(p.badge), ' ', p.admin && el('span', { class: 'tag admin' }, 'Admin'), ' ', p.verified && el('span', { class: 'tag verified' }, 'Verified'), ' ', p.hat_maker && el('span', { class: 'tag verified' }, 'Hat-Maker')),
        el('div', { class: 'soft' }, `#${p.number ?? 0} · `, days < 1 ? 'Joined today' : `Joined ${ago(p.joined_secs_ago)}`),
        (action || (session && !isMe)) && el('div', { style: 'margin-top:8px' }, action, ' ',
          session && !isMe && p.games.length > 0 && followButton('creator', p.player_id, p.name), ' ',
          session && !isMe && el('button', { class: 'chip', title: 'Report', onclick: () => startReport('player', p.player_id, p.name) }, icon(ICON.flag)))),
      bodyPicture(p.player_id, p.look)),
    reportPanel('player', p.player_id),
    p.about && el('div', { class: 'about' }, p.about),
    el('h2', { class: 'section' }, `Games (${p.games.length})`),
    p.games.length
      ? el('div', { class: 'games' }, p.games.map((g) => gameCard(entries().find((e) => e.key === g.game_id) || {
        key: g.game_id, number: g.number, title: g.title, by: g.author, servers: [], playing: 0, symbol: ICON.globe,
      })))
      : el('p', { class: 'soft' }, isMe ? 'Games you publish from the app show up here.' : 'No published games yet.'),
    badgesOf(p),
    favoritesOf(p),
    (p.groups || []).length > 0 && el('h2', { class: 'section' }, `Groups (${p.groups.length})`),
    (p.groups || []).length > 0 && el('div', { class: 'group-list' }, p.groups.map((g) => el('a', { class: 'chip', href: `/group/${g.group_id}` }, badge(g), ' ', g.name))),
  ];
}

const badgeLists = new Map();

function loadBadges(path) {
  if (!badgeLists.has(path)) {
    badgeLists.set(path, null);
    api(path)
      .then((list) => badgeLists.set(path, list))
      .catch(() => badgeLists.set(path, []))
      .finally(() => render());
  }
  return badgeLists.get(path);
}

function emblem(b, size = 56) {
  const base = GROUP_COLORS[b.color] || GROUP_COLORS.purple;
  const mix = (by) => rgb(base.map((v) => Math.round(by >= 0 ? v + (255 - v) * by : v * (1 + by))));
  const hex = (r) => Array.from({ length: 6 }, (_, i) => {
    const a = (Math.PI / 3) * i - Math.PI / 2;
    return `${(50 + r * Math.cos(a)).toFixed(2)},${(50 + r * Math.sin(a)).toFixed(2)}`;
  }).join(' ');
  const id = `b${fnv(`${b.badge_id}${size}`)}`;
  return svg('svg', { viewBox: '0 0 100 100', width: size, height: size, class: 'emblem', 'aria-hidden': 'true' },
    svg('defs', {}, svg('linearGradient', { id, x1: 0, y1: 0, x2: 0, y2: 1 },
      svg('stop', { offset: '0', 'stop-color': mix(0.22) }), svg('stop', { offset: '1', 'stop-color': mix(-0.1) }))),
    svg('polygon', { points: hex(48), fill: mix(-0.35) }),
    svg('polygon', { points: hex(41), fill: `url(#${id})` }),
    svg('polygon', { points: hex(35), fill: 'none', stroke: mix(0.45), 'stroke-width': 3 }),
    svg('text', { x: 50, y: 51, 'text-anchor': 'middle', 'dominant-baseline': 'central', 'font-family': 'Phosphor', 'font-size': 40, fill: '#fff' }, BADGE_GLYPHS[b.icon] || BADGE_GLYPHS.medal));
}

function badgeShelf(list, about) {
  return el('div', { class: 'badge-shelf' }, list.map((b) => el('div', { class: 'badge-card', title: `${b.name}\n${about(b)}` },
    emblem(b), el('b', {}, b.name), el('small', { class: 'soft' }, about(b)))));
}

function badgesOf(p) {
  const list = loadBadges(`/accounts/${p.player_id}/badges`);
  if (!list?.length) return null;
  return [el('h2', { class: 'section' }, `Badges (${list.length})`), badgeShelf(list, (b) => b.game_title || b.description)];
}

function gameBadges(key) {
  const list = loadBadges(`/games/${encodeURIComponent(key)}/badges`);
  if (!list?.length) return null;
  return [el('h2', { class: 'section' }, 'Badges'), badgeShelf(list, (b) => `${b.description}${b.description ? ' · ' : ''}${count(b.earned_by)} ${b.earned_by === 1 ? 'player has' : 'players have'} it`)];
}

const favorites = new Map();

function favoritesOf(p) {
  const keys = favorites.get(p.player_id);
  if (!keys) {
    favorites.set(p.player_id, []);
    api(`/accounts/${p.player_id}/favorites`)
      .then((list) => { favorites.set(p.player_id, list); if (route.name === 'user') render(); })
      .catch(() => {});
    return null;
  }
  const shown = keys.map((k) => entries().find((e) => e.key === k)).filter(Boolean);
  if (!shown.length) return null;
  return [el('h2', { class: 'section' }, `Favorites (${shown.length})`), el('div', { class: 'games' }, shown.map(gameCard))];
}

// --- Groups -----------------------------------------------------------------

const GROUP_COLORS = { red: [214, 48, 49], orange: [230, 126, 34], yellow: [212, 172, 13], green: [39, 174, 96], teal: [22, 160, 133], blue: [41, 128, 185], navy: [36, 59, 120], purple: [120, 80, 200], pink: [214, 72, 150], brown: [133, 87, 50], grey: [110, 118, 125], black: [30, 30, 35] };
const MAX_TAG = 5;
const JOIN_NAMES = { open: 'Anyone can join', request: 'Ask to join', invite: 'Invite only' };
const badge = (g) => el('span', { class: 'tag group', title: g.name, style: `background-color:${rgb(GROUP_COLORS[g.color] || GROUP_COLORS.purple)}` }, g.tag);
const pickFrom = (options, chosen) => el('select', {}, Object.entries(options).map(([value, label]) => el('option', { value, selected: value === chosen }, label)));
const colorPick = (chosen) => pickFrom(Object.fromEntries(Object.keys(GROUP_COLORS).map((c) => [c, c[0].toUpperCase() + c.slice(1)])), chosen || 'purple');
let groupInvites = null;
const groupPages = new Map();
let groupList = { text: '', results: null };

async function changeGroup(action, groupId, fields, done) {
  const time = Math.max(Date.now(), lastSigned.group + 1);
  lastSigned.group = time;
  const c = { player_id: session.playerId, action, group_id: groupId, name: '', tag: '', about: '', other_id: '', role: null, text: '', join: null, color: '', time, ...fields };
  c.signature = signText(secretKey(), `libroxel group\n${c.player_id}\n${c.action}\n${c.group_id}\n${c.name}\n${c.tag}\n${c.about}\n${c.other_id}\n${c.role || ''}\n${c.text}\n${c.join || ''}\n${c.color}\n${c.time}`);
  let answer = null;
  try {
    answer = await api('/groups', { method: 'POST', body: c });
    if (done) flash = { text: done };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  groupPages.delete(groupId);
  groupList.results = null;
  groupInvites = null;
  if (session) data.me = await api(`/accounts/${session.playerId}`).catch(() => data.me);
  render();
  return answer;
}

function groupRow(g) {
  return el('div', { class: 'person' },
    el('div', { class: 'what' },
      el('a', { href: `/group/${g.group_id}` }, el('b', {}, g.name), ' ', badge(g)),
      el('small', { class: 'soft' }, `${count(g.members)} ${g.members === 1 ? 'member' : 'members'} · owned by ${g.owner} · ${JOIN_NAMES[g.join] || JOIN_NAMES.open}`)));
}

function groupsPage() {
  if (!groupList.results) {
    const text = groupList.text;
    groupList.results = [];
    api('/groups', { query: { q: text } })
      .then((r) => { if (groupList.text === text) groupList.results = r; })
      .catch((e) => { flash = { text: e.message, kind: 'error' }; })
      .finally(() => route.name === 'groups' && render());
  }
  const box = el('input', { type: 'search', placeholder: 'A group name or tag', value: groupList.text });
  const find = () => { groupList = { text: box.value.trim(), results: null }; render(); };
  box.addEventListener('keydown', (e) => e.key === 'Enter' && find());
  const left = [
    el('div', { class: 'search' }, box, el('button', { class: 'chip', onclick: find }, icon(ICON.search), 'Find')),
    groupList.results.length ? groupList.results.map(groupRow) : el('p', { class: 'soft' }, groupList.text ? `No groups called “${groupList.text}”.` : 'No groups yet.'),
  ];
  const right = [];
  if (session && groupInvites === null) {
    groupInvites = [];
    const time = Math.max(Date.now(), lastSigned.group + 1);
    lastSigned.group = time;
    const signature = signText(secretKey(), `libroxel group invites\n${session.playerId}\n${time}`);
    api(`/groups/invites/${session.playerId}`, { query: { time, signature } })
      .then((r) => { groupInvites = r; if (route.name === 'groups' && r.length) render(); })
      .catch(() => {});
  }
  if (session && groupInvites?.length) {
    right.push(el('h2', { class: 'subhead sun' }, `Invites (${groupInvites.length})`));
    for (const g of groupInvites) {
      right.push(el('div', { class: 'person' },
        el('div', { class: 'what' }, el('a', { href: `/group/${g.group_id}` }, el('b', {}, g.name), ' ', badge(g))),
        el('div', { class: 'buttons' },
          el('button', { class: 'btn go small', onclick: () => changeGroup('join', g.group_id, {}, `You joined ${g.name}.`) }, icon(ICON.check), 'Join'),
          el('button', { class: 'chip', onclick: () => changeGroup('leave', g.group_id, {}, 'Declined.') }, 'Decline'))));
    }
  }
  right.push(el('h2', { class: 'subhead' }, 'Make a group'));
  if (!session) right.push(el('p', { class: 'soft' }, el('a', { href: '/sign-in' }, 'Sign in'), ' to make or join a group.'));
  else {
    const name = el('input', { placeholder: 'Name', maxlength: 30 });
    const tag = el('input', { placeholder: `Tag (2 to ${MAX_TAG})`, maxlength: MAX_TAG, style: 'text-transform:uppercase' });
    const color = colorPick('purple');
    const join = pickFrom(JOIN_NAMES, 'open');
    const about = el('textarea', { placeholder: 'About', maxlength: 1000, rows: 3 });
    const make = async () => {
      const made = await changeGroup('create', '', { name: name.value.trim(), tag: tag.value.trim().toUpperCase(), color: color.value, join: join.value, about: about.value.trim() }, `Made ${name.value.trim()}.`);
      if (made?.group_id) goTo(`/group/${made.group_id}`);
    };
    right.push(el('div', { class: 'form' }, name, el('div', { class: 'search' }, tag, color), join, about, el('div', { class: 'form-actions' }, el('button', { class: 'btn go', onclick: make }, icon(ICON.check), 'Make'))));
  }
  return [el('h1', { class: 'title' }, 'Groups'), el('div', { class: 'columns' }, el('div', {}, left), el('div', {}, right))];
}

const ROLE_NAMES = { owner: 'Owner', admin: 'Admin', member: 'Member' };
const PERMISSIONS = [['games', 'Games'], ['funds', 'Funds'], ['wall', 'Wall'], ['members', 'Members']];
const FUND_REASONS = { earned: 'Sale', engagement: 'Play time' };

function groupPage() {
  const id = route.id;
  const back = el('a', { class: 'back', href: '/groups' }, icon(ICON.left), 'Groups');
  const known = groupPages.get(id);
  if (!known) {
    let query;
    if (session) {
      const time = Math.max(Date.now(), lastSigned.group + 1);
      lastSigned.group = time;
      query = { viewer: session.playerId, time, signature: signText(secretKey(), `libroxel group view\n${session.playerId}\n${id}\n${time}`) };
    }
    api(`/groups/${encodeURIComponent(id)}`, { query })
      .then((g) => groupPages.set(id, g))
      .catch(() => groupPages.set(id, { missing: true }))
      .finally(() => route.name === 'group' && route.id === id && render());
    return [back, el('p', { class: 'soft' }, 'Loading…')];
  }
  if (known.missing) return [back, el('div', { class: 'empty' }, "That group isn't here (any more).")];
  const { group, members, posts, games, requests = [], invites = [] } = known;
  const me = session && members.find((m) => m.player_id === session.playerId);
  const role = me?.role;
  const can = new Set(known.can || []);
  const shown = data.me?.badge?.group_id === id;
  const actions = [];
  if (session && !me) {
    if (known.invited) {
      actions.push(el('button', { class: 'btn go small', onclick: () => changeGroup('join', id, {}, `You joined ${group.name}.`) }, icon(ICON.check), 'Accept invite'));
      actions.push(el('button', { class: 'chip', onclick: () => changeGroup('leave', id, {}, 'Declined.') }, 'Decline'));
    } else if (known.requested) {
      actions.push(el('span', { class: 'soft' }, 'Request sent'), el('button', { class: 'chip', onclick: () => changeGroup('leave', id, {}, 'Request taken back.') }, 'Cancel'));
    } else if (group.join === 'request') {
      actions.push(el('button', { class: 'btn go small', onclick: () => changeGroup('join', id, {}, `Asked to join ${group.name}.`) }, icon(ICON.userPlus), 'Ask to join'));
    } else if (group.join === 'invite') {
      actions.push(el('span', { class: 'soft' }, 'Invite only'));
    } else {
      actions.push(el('button', { class: 'btn go small', onclick: () => changeGroup('join', id, {}, `You joined ${group.name}.`) }, icon(ICON.userPlus), 'Join'));
    }
  }
  if (session && !me) actions.push(el('button', { class: 'link', title: 'Report', onclick: () => reportGroup(group) }, icon(ICON.flag)));
  if (me) {
    actions.push(el('button', { class: 'chip', onclick: () => changeGroup('show', shown ? '' : id, {}, shown ? 'Your badge is hidden.' : `Your badge is ${group.tag} now.`) }, shown ? 'Hide badge' : 'Show badge'));
    actions.push(el('button', {
      class: 'chip', onclick: (e) => {
        const b = e.currentTarget;
        if (b.dataset.sure) changeGroup('leave', id, {}, `You left ${group.name}.`);
        else { b.dataset.sure = '1'; b.textContent = 'Leave?'; }
      },
    }, 'Leave'));
  }
  const out = [
    back,
    el('div', { class: 'hello' },
      el('div', {},
        el('h1', {}, group.name, ' ', badge(group)),
        el('div', { class: 'soft' }, `${count(group.members)} ${group.members === 1 ? 'member' : 'members'} · owned by `, el('a', { href: `/user/${group.owner_id}` }, group.owner), ` · ${JOIN_NAMES[group.join] || JOIN_NAMES.open}`),
        actions.length > 0 && el('div', { class: 'buttons', style: 'margin-top:8px' }, actions))),
    group.about && el('div', { class: 'about' }, group.about),
  ];
  if (role === 'owner') {
    const name = el('input', { value: group.name, maxlength: 30 });
    const tag = el('input', { value: group.tag, maxlength: MAX_TAG });
    const color = colorPick(group.color);
    const join = pickFrom(JOIN_NAMES, group.join || 'open');
    const about = el('textarea', { rows: 3, maxlength: 1000 }, group.about);
    out.push(el('details', {}, el('summary', {}, 'Edit'), el('div', { class: 'form' }, name, el('div', { class: 'search' }, tag, color), join, about,
      el('div', { class: 'form-actions' }, el('button', { class: 'btn go small', onclick: () => changeGroup('edit', id, { name: name.value.trim(), tag: tag.value.trim().toUpperCase(), color: color.value, join: join.value, about: about.value.trim() }, 'Saved.') }, icon(ICON.check), 'Save')))));
  }
  if (can.has('members')) {
    const who = el('input', { placeholder: 'Their name' });
    const invite = async () => {
      const found = await api(`/accounts/${encodeURIComponent(who.value.trim())}`).catch(() => null);
      if (!found) { flash = { text: `Nobody is called ${who.value.trim()}.`, kind: 'error' }; render(); return; }
      changeGroup('invite', id, { other_id: found.player_id }, `Invited ${found.name}.`);
    };
    who.addEventListener('keydown', (e) => e.key === 'Enter' && invite());
    out.push(el('div', { class: 'search' }, who, el('button', { class: 'chip', onclick: invite }, icon(ICON.userPlus), 'Invite')));
    for (const p of requests) {
      out.push(el('div', { class: 'person' },
        el('div', { class: 'what' }, el('a', { href: `/user/${p.number ?? p.player_id}` }, el('b', {}, p.name)), el('small', { class: 'soft' }, 'Wants to join')),
        el('div', { class: 'buttons' },
          el('button', { class: 'btn go small', onclick: () => changeGroup('accept', id, { other_id: p.player_id }, `${p.name} joined.`) }, icon(ICON.check), 'Accept'),
          el('button', { class: 'chip', onclick: () => changeGroup('decline', id, { other_id: p.player_id }, 'Declined.') }, 'Decline'))));
    }
    for (const p of invites) {
      out.push(el('div', { class: 'person' },
        el('div', { class: 'what' }, el('b', {}, p.name), el('small', { class: 'soft' }, 'Invited')),
        el('div', { class: 'buttons' }, el('button', { class: 'chip', onclick: () => changeGroup('decline', id, { other_id: p.player_id }, 'Invite taken back.') }, 'Cancel'))));
    }
  }

  const wall = [el('h2', { class: 'subhead' }, 'Wall')];
  if (me) {
    const box = el('textarea', { placeholder: 'Say something', maxlength: 300, rows: 2 });
    wall.push(el('div', { class: 'form' }, box, el('div', { class: 'form-actions' }, el('button', { class: 'btn go small', onclick: () => box.value.trim() && changeGroup('post', id, { text: box.value.trim() }) }, 'Post'))));
  }
  if (!posts.length) wall.push(el('p', { class: 'soft' }, 'Nothing here yet.'));
  for (const post of posts) {
    const mine = session?.playerId === post.player_id;
    wall.push(el('div', { class: 'post' },
      el('div', {}, el('a', { href: `/user/${post.player_id}` }, el('b', {}, post.name)), el('span', { class: 'soft' }, ` · ${ago(post.secs_ago)}`),
        (mine || can.has('wall')) && el('button', { class: 'link', title: 'Remove', onclick: () => changeGroup('unpost', id, { other_id: String(post.post_id) }) }, icon(ICON.x)),
        session && !mine && el('button', { class: 'link', title: 'Report', onclick: () => reportPost(post) }, icon(ICON.flag))),
      el('div', {}, post.text)));
  }

  const side = [el('h2', { class: 'subhead' }, `Members (${members.length})`)];
  for (const m of members) {
    const controls = [];
    if (role === 'owner' && m.role !== 'owner') {
      const pick = el('select', {}, ['member', 'admin', 'owner'].map((r) => el('option', { value: r, selected: r === m.role }, ROLE_NAMES[r])));
      pick.addEventListener('change', () => changeGroup('role', id, { other_id: m.player_id, role: pick.value }, `${m.name} is ${ROLE_NAMES[pick.value].toLowerCase()} now.`));
      controls.push(pick);
      const boxes = PERMISSIONS.map(([p, label]) => {
        const box = el('input', { type: 'checkbox', checked: (m.can || []).includes(p), disabled: m.role === 'admin' && p !== 'funds' });
        box.dataset.p = p;
        return el('label', {}, box, ` ${label}`);
      });
      const save = () => changeGroup('permissions', id, { other_id: m.player_id, text: boxes.map((b) => b.firstChild).filter((b) => b.checked && !b.disabled).map((b) => b.dataset.p).join(' ') }, 'Saved.');
      controls.push(el('details', {}, el('summary', {}, 'Permissions'), el('div', { class: 'form' }, boxes, el('button', { class: 'chip', onclick: save }, icon(ICON.check), 'Save'))));
    }
    const rank = { owner: 2, admin: 1, member: 0 };
    if (can.has('members') && rank[m.role] < rank[role]) controls.push(el('button', { class: 'chip iconbtn', title: `Remove ${m.name}`, onclick: () => changeGroup('kick', id, { other_id: m.player_id }, `Removed ${m.name}.`) }, icon(ICON.userMinus)));
    side.push(el('div', { class: 'person' },
      portrait(m.player_id, 36),
      el('div', { class: 'what' }, el('a', { href: `/user/${m.number ?? m.player_id}` }, el('b', {}, m.name)),
        el('small', { class: 'soft' }, m.role === 'owner' ? [icon(ICON.crown), ' Owner'] : m.role === 'admin' ? [icon(ICON.shield), ' Admin'] : 'Member')),
      el('div', { class: 'buttons' }, controls)));
  }

  const gameList = [el('h2', { class: 'section' }, `Games (${games.length})`)];
  gameList.push(games.length
    ? el('div', { class: 'games' }, games.map((g) => gameCard(entries().find((e) => e.key === g.game_id) || { key: g.game_id, number: g.number, title: g.title, by: group.name, servers: [], playing: 0, symbol: ICON.globe })))
    : el('p', { class: 'soft' }, 'No games yet.'));
  if (can.has('games')) {
    const theirs = (data.me?.games || []).filter((g) => g.author_id === session.playerId && !games.some((x) => x.game_id === g.game_id));
    if (theirs.length) {
      const pick = el('select', {}, theirs.map((g) => el('option', { value: g.game_id }, g.title)));
      gameList.push(el('div', { class: 'search' }, pick, el('button', { class: 'chip', onclick: () => changeGroup('add_game', id, { other_id: pick.value }, 'Added.') }, 'Add game')));
    }
    if (games.length && role === 'owner') {
      const pick = el('select', {}, games.map((g) => el('option', { value: g.game_id }, g.title)));
      gameList.push(el('div', { class: 'search' }, pick,
        el('button', { class: 'chip', onclick: () => goTo(`/creator/${encodeURIComponent(pick.value)}/data`) }, 'Data'),
        el('button', { class: 'chip', onclick: () => changeGroup('remove_game', id, { other_id: pick.value }, 'Removed.') }, 'Remove game')));
    }
  }
  const funds = [];
  if (known.funds) {
    const f = known.funds;
    funds.push(el('h2', { class: 'section' }, `Funds · ${credits(f.balance)}`));
    const who = el('select', {}, members.map((m) => el('option', { value: m.player_id }, m.name)));
    const amount = el('input', { type: 'number', min: 1, max: f.balance, placeholder: 'Credits', style: 'width:100px' });
    funds.push(el('div', { class: 'search' }, who, amount, el('button', { class: 'chip', onclick: () => {
      const m = members.find((x) => x.player_id === who.value);
      changeGroup('pay', id, { other_id: who.value, text: String(Number(amount.value) || 0) }, `Paid ${m?.name}.`);
    } }, 'Pay')));
    const said = (h) => h.reason === 'group pay' ? `Paid ${h.to}` : FUND_REASONS[h.reason] || h.reason;
    for (const h of f.history) {
      funds.push(el('div', { class: 'person' },
        el('div', { class: 'what' }, el('b', {}, said(h)), el('small', { class: 'soft' }, [h.by && `${h.by} · `, ago(h.secs_ago)])),
        el('div', {}, `${h.amount > 0 ? '+' : ''}${count(h.amount)}`)));
    }
  }
  return [...out, ...gameList, ...funds, el('div', { class: 'columns' }, el('div', {}, wall), el('div', {}, side))];
}

async function reportGroup(group) {
  const time = nowSecs();
  const reason = 'Not OK';
  const signature = signText(secretKey(), `libroxel report\n${session.playerId}\ngroup\n${group.group_id}\n${reason}\n${time}`);
  try {
    await api('/reports', { method: 'POST', body: { reporter_id: session.playerId, kind: 'group', target: group.group_id, reason, time, signature } });
    flash = { text: 'Reported. Thanks.' };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

async function reportPost(post) {
  const time = nowSecs();
  const target = String(post.post_id);
  const reason = 'Not OK';
  const signature = signText(secretKey(), `libroxel report\n${session.playerId}\npost\n${target}\n${reason}\n${time}`);
  try {
    await api('/reports', { method: 'POST', body: { reporter_id: session.playerId, kind: 'post', target, reason, time, signature } });
    flash = { text: 'Reported. Thanks.' };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

// --- Stats -------------------------------------------------------------------

const usage = { days: 30, result: null, error: null, loading: false, for: null };

function loadUsage() {
  const key = `${session.playerId}/${usage.days}`;
  if (usage.loading || usage.for === key) return;
  usage.loading = true;
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel admin\n${session.playerId}\n${time}`);
  api(`/admin/${session.playerId}`, { query: { time, signature, days: usage.days } })
    .then((r) => { usage.result = r; usage.error = null; })
    .catch((e) => { usage.error = e.message; })
    .finally(() => { usage.loading = false; usage.for = key; if (route.name === 'stats') render(); });
}

function statsPage() {
  if (!session) return needSignIn('Sign in to see the stats.');
  loadUsage();
  const out = [el('h1', { class: 'title' }, 'Stats'),
    el('div', { class: 'hats' }, [7, 30, 90].map((d) =>
      el('button', { class: `chip${usage.days === d ? ' on' : ''}`, onclick: () => { usage.days = d; usage.for = null; render(); } }, `${d} days`)))];
  if (usage.error) return [...out, status(usage.error, 'error')];
  const u = usage.result;
  if (!u) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  const today = u.days[u.days.length - 1] || {};
  const tiles = [[count(today.users || 0), 'Today'], [count(u.online), 'Online now'], [count(u.users), 'Users'], [count(u.sessions), 'Sessions'], [count(u.crashes), 'Crashes']];
  out.push(el('div', { class: 'tiles' }, tiles.map(([v, l]) => el('div', {}, el('b', {}, v), el('span', { class: 'caps' }, l)))));
  const daily = (key, label, f = (v) => v) => chart(u.days.map((d) => ({ v: f(d[key]), short: dayName(d.day), long: dayName(d.day) })), { bars: true, label });
  out.push(el('h2', { class: 'section' }, 'Daily users'), daily('users', 'Users each day'));
  out.push(el('h2', { class: 'section' }, 'New users'), daily('new_users', 'New users each day'));
  out.push(el('h2', { class: 'section' }, 'Sessions'), daily('sessions', 'Sessions each day'));
  out.push(el('h2', { class: 'section' }, 'Session length (min)'), daily('average_session_secs', 'Average session minutes each day', (s) => Math.round(s / 60)));
  out.push(el('h2', { class: 'section' }, 'Crashes'), daily('crashes', 'Crashes each day'));
  const shares = (title, list) => el('div', {}, el('h2', { class: 'section' }, title),
    list.length ? el('table', { class: 'numbers' }, el('tbody', {}, list.map((x) => el('tr', {}, el('td', {}, x.name || '?'), el('td', {}, count(x.users)), el('td', {}, percent(u.users ? x.users / u.users : null))))))
      : el('p', { class: 'soft' }, 'None'));
  out.push(el('div', { class: 'columns' }, shares('Versions', u.versions), shares('Platforms', u.platforms)));
  out.push(el('h2', { class: 'section' }, 'Top crashes'));
  if (!u.top_crashes.length) out.push(el('div', { class: 'empty' }, 'No crashes.'));
  for (const c of u.top_crashes) {
    out.push(el('div', { class: 'report-card' },
      el('div', {}, el('b', {}, c.title), el('span', { class: 'soft' }, ` · ${count(c.count)} ${c.count === 1 ? 'time' : 'times'} · ${count(c.users)} ${c.users === 1 ? 'user' : 'users'} · ${clock(c.last)}`)),
      el('div', { class: 'soft' }, [...c.versions, ...c.platforms].join(' · ')),
      el('details', {}, el('summary', {}, 'Details'), el('pre', {}, c.text))));
  }
  return out;
}

// --- Moderation -------------------------------------------------------------

const mod = { board: null, error: null, busy: false, for: null, at: 0 };
const HOURS = [[1, '1 hour'], [24, '1 day'], [168, '7 days'], [720, '30 days']];
const KINDS = { player: 'Player', game: 'Game', item: 'Item', market: 'Marketplace', comment: 'Comment', message: 'Message', group: 'Group', post: 'Post', appeal: 'Appeal' };
const PENALTIES = { ban: 'Ban', mute: 'Mute', warning: 'Warning' };

function loadModeration() {
  mod.for = session.playerId;
  mod.at = Date.now();
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel moderation\n${session.playerId}\n${time}`);
  api(`/moderation/${session.playerId}`, { query: { time, signature } })
    .then((b) => { mod.board = b; mod.error = null; })
    .catch((e) => { mod.error = e.message; })
    .finally(() => route.name === 'moderation' && render());
}

async function moderate(action, fields, done) {
  const time = Math.max(Date.now(), lastSigned.moderate + 1);
  lastSigned.moderate = time;
  const c = { moderator_id: session.playerId, action, kind: '', target: '', player_id: '', reason: '', hours: 0, time, ...fields };
  c.signature = signText(secretKey(), `libroxel moderate\n${c.moderator_id}\n${c.action}\n${c.kind}\n${c.target}\n${c.player_id}\n${c.reason}\n${c.hours}\n${c.time}`);
  mod.busy = true;
  render();
  try {
    await api('/moderation', { method: 'POST', body: c });
    flash = { text: done };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  mod.busy = false;
  loadModeration();
}

const lasts = (secs) => (secs === null || secs === undefined ? 'Forever' : secs < 3600 ? `${Math.ceil(secs / 60)} min` : secs < 172800 ? `${Math.ceil(secs / 3600)} h` : `${Math.ceil(secs / 86400)} days`);
const pick = (list) => el('select', {}, list.map(([h, label]) => el('option', { value: h }, label)));
const playerLink = (id, name) => el('a', { href: `/user/${id}` }, el('b', {}, name || id.slice(0, 8)));

function placeOf(q) {
  if (q.text === null) return null;
  if (q.kind === 'game') return gameHrefOf(q.target);
  if (q.kind === 'comment') return gameHrefOf(q.place);
  if (q.kind === 'group' || q.kind === 'post') return `/group/${q.place}`;
  return null;
}

function reportCard(q) {
  const reason = el('input', { type: 'text', placeholder: 'Reason', maxlength: 300 });
  const muteFor = pick(HOURS);
  const banFor = pick([...HOURS, [0, 'Forever']]);
  const step = (action, extra, done) => moderate(action, { kind: q.kind, target: q.target, reason: reason.value.trim(), ...extra }, done);
  const href = placeOf(q);
  const n = q.reports.length;
  return el('div', { class: 'report-card' },
    el('div', {},
      el('span', { class: 'tag kind' }, KINDS[q.kind] || q.kind), ' ',
      q.author_id ? playerLink(q.author_id, q.author) : el('span', { class: 'soft' }, 'Gone'),
      el('span', { class: 'soft' }, ` · ${n} ${n === 1 ? 'report' : 'reports'}`),
      q.hidden && el('span', { class: 'tag admin', style: 'margin-left:6px' }, 'Hidden')),
    q.text !== null && q.kind !== 'player' && el('div', { class: 'text' }, href ? el('a', { href }, q.text) : q.text),
    el('ul', { class: 'reasons soft' }, q.reports.map((r) => el('li', {}, `${r.reporter || 'Someone'}: ${r.reason} · ${ago(r.secs_ago)}`,
      r.evidence && el('pre', {}, r.evidence),
      (r.chat || []).length > 0 && el('details', {}, el('summary', {}, `Chat (${r.chat.length})`),
        el('pre', {}, r.chat.map((l) => `${l.reported ? '> ' : ''}${l.from || l.from_id.slice(0, 8)}: ${l.text}`).join('\n')))))),
    el('div', { class: 'buttons' },
      reason,
      q.text !== null && q.kind !== 'player' && el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('remove', {}, 'Removed.') }, icon(ICON.trash), 'Remove'),
      el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('dismiss', {}, 'Dismissed.') }, icon(ICON.check), 'Dismiss')),
    q.author_id && el('div', { class: 'buttons' },
      el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('warn', {}, 'Warned.') }, icon(ICON.bell), 'Warn'),
      muteFor,
      el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('mute', { hours: Number(muteFor.value) }, 'Muted.') }, icon(ICON.chat), 'Mute'),
      banFor,
      el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('ban', { hours: Number(banFor.value) }, 'Banned.') }, icon(ICON.lock), 'Ban')));
}

function penaltyRow(p, action, done) {
  return el('div', { class: 'person' },
    el('div', { class: 'what' }, playerLink(p.player_id, p.name),
      el('small', { class: 'soft' }, `${p.reason} · ${lasts(p.left_secs)}`)),
    el('div', { class: 'buttons' }, el('button', { class: 'chip', disabled: mod.busy, onclick: () => moderate(action, { player_id: p.player_id }, done) }, 'Lift')));
}

function appealCard(a) {
  const reply = el('input', { type: 'text', placeholder: 'Reply', maxlength: 300 });
  const step = (action, done) => moderate(action, { kind: 'appeal', target: String(a.appeal_id), reason: reply.value.trim() }, done);
  const left = a.kind === 'warning' ? '' : ` · ${lasts(a.left_secs)} left`;
  return el('div', { class: 'report-card' },
    el('div', {}, el('span', { class: 'tag kind' }, PENALTIES[a.kind] || a.kind), ' ', playerLink(a.player_id, a.name),
      el('span', { class: 'soft' }, ` · ${ago(a.secs_ago)}${left}`)),
    el('div', { class: 'soft' }, a.penalty),
    el('div', { class: 'text' }, a.text),
    a.history.length > 0 && el('details', {}, el('summary', {}, `History (${a.history.length})`), a.history.map(logRow)),
    el('div', { class: 'buttons' },
      reply,
      el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('accept', 'Lifted.') }, icon(ICON.check), 'Lift'),
      el('button', { class: 'chip', disabled: mod.busy, onclick: () => step('deny', 'Kept.') }, icon(ICON.lock), 'Keep')));
}

function logRow(l) {
  return el('div', { class: 'post' },
    el('b', {}, l.moderator || 'Hub'), ` ${l.action}`,
    l.player_id && [' ', playerLink(l.player_id, l.player)],
    l.kind && el('span', { class: 'soft' }, ` ${KINDS[l.kind] || l.kind}`),
    l.hours > 0 && el('span', { class: 'soft' }, ` ${lasts(l.hours * 3600)}`),
    el('span', { class: 'soft' }, ` · ${ago(l.secs_ago)}`),
    l.reason && el('div', {}, l.reason),
    l.text && el('div', { class: 'soft' }, l.text));
}

/** Who may add hats to the catalog: owners give Hat-Maker and take it away. */
function hatMakersPanel(makers) {
  const name = el('input', { type: 'text', placeholder: 'Account name', maxlength: 40 });
  const add = () => {
    const who = name.value.trim();
    if (who) moderate('hat_maker', { player_id: who }, `${who} is a Hat-Maker.`);
  };
  name.addEventListener('keydown', (e) => { if (e.key === 'Enter') add(); });
  return el('div', {},
    el('h2', { class: 'section' }, `Hat-Makers (${makers.length})`),
    el('p', { class: 'note' }, 'Only Hat-Makers (and owners) can add hats to the catalog.'),
    el('div', { class: 'buttons' }, name, el('button', { class: 'chip', disabled: mod.busy, onclick: add }, icon(ICON.userPlus), 'Make Hat-Maker')),
    makers.map((m) => el('div', { class: 'person' },
      el('div', { class: 'what' }, playerLink(m.player_id, m.name)),
      el('div', { class: 'buttons' }, el('button', { class: 'chip', disabled: mod.busy, onclick: () => moderate('no_hat_maker', { player_id: m.player_id }, `${m.name || 'They'} can no longer make hats.`) }, 'Take away')))));
}

function moderationPage() {
  if (!session) return needSignIn('Sign in to moderate.');
  if (mod.for !== session.playerId || (!mod.busy && Date.now() - mod.at > 30000)) loadModeration();
  const out = [el('h1', { class: 'title' }, 'Moderation')];
  if (mod.error) return [...out, status(mod.error, 'error')];
  const b = mod.board;
  if (!b) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  const appeals = b.appeals || [];
  out.push(el('h2', { class: 'section' }, `Appeals (${appeals.length})`));
  out.push(appeals.length ? appeals.map(appealCard) : el('div', { class: 'empty' }, 'No appeals.'));
  out.push(el('h2', { class: 'section' }, `Reports (${b.reports.length})`));
  out.push(b.reports.length ? b.reports.map(reportCard) : el('div', { class: 'empty' }, 'Nothing reported.'));
  out.push(el('div', { class: 'columns' },
    el('div', {}, el('h2', { class: 'subhead' }, `Banned (${b.banned.length})`), b.banned.map((p) => penaltyRow(p, 'unban', 'Unbanned.'))),
    el('div', {}, el('h2', { class: 'subhead' }, `Muted (${b.muted.length})`), b.muted.map((p) => penaltyRow(p, 'unmute', 'Unmuted.')))));
  out.push(hatMakersPanel(b.hat_makers || []));
  out.push(el('h2', { class: 'section' }, 'Log'));
  out.push(b.log.length ? b.log.map(logRow) : el('p', { class: 'soft' }, 'Nothing yet.'));
  return out;
}

const appealing = { key: null, text: '', busy: false };

async function sendAppeal(kind, warningId) {
  const time = Date.now();
  const text = appealing.text.trim();
  const signature = signText(secretKey(), `libroxel appeal\n${session.playerId}\n${kind}\n${warningId}\n${text}\n${time}`);
  appealing.busy = true;
  render();
  try {
    await api('/appeals', { method: 'POST', body: { player_id: session.playerId, kind, warning_id: warningId, text, time, signature } });
    flash = { text: 'Sent. A moderator will look at it.' };
    appealing.key = null;
    appealing.text = '';
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  appealing.busy = false;
  securityInfo = null;
  loadSecurity();
  render();
}

function appealRow(kind, warningId, label, appeals) {
  const key = `${kind}:${warningId}`;
  const sent = appeals.find((a) => a.kind === kind && (kind !== 'warning' || a.warning_id === warningId));
  const said = { open: 'Appeal sent', accepted: 'Appeal accepted', denied: 'Appeal denied' };
  let tail;
  if (sent) tail = el('span', { class: 'soft' }, ` · ${said[sent.state] || sent.state}${sent.reply ? `: ${sent.reply}` : ''}`);
  else if (appealing.key !== key) tail = el('button', { class: 'link', onclick: () => { appealing.key = key; appealing.text = ''; render(); } }, 'Appeal');
  const row = [el('div', {}, label, ' ', tail)];
  if (!sent && appealing.key === key) {
    const box = el('textarea', { rows: 3, maxlength: 1000, placeholder: 'Why should it be lifted?' }, appealing.text);
    box.addEventListener('input', () => { appealing.text = box.value; });
    row.push(box, el('div', { class: 'buttons' },
      el('button', { class: 'btn small', disabled: appealing.busy, onclick: () => sendAppeal(kind, warningId) }, 'Send'),
      el('button', { class: 'chip', onclick: () => { appealing.key = null; render(); } }, 'Cancel')));
  }
  return el('div', { class: 'report-card' }, row);
}

function standingPanel() {
  const st = securityInfo?.standing;
  if (!st || (!st.banned && !st.muted && !st.warnings.length && !st.appeals.length)) return null;
  const out = [el('h2', { class: 'section', id: 'standing' }, 'Standing')];
  if (st.banned) out.push(appealRow('ban', 0, `Banned: ${st.banned.reason} · ${lasts(st.banned.left_secs)}`, st.appeals));
  if (st.muted) out.push(appealRow('mute', 0, `Muted: ${st.muted.reason || 'no reason given'} · ${lasts(st.muted.left_secs)}`, st.appeals));
  for (const w of st.warnings) out.push(appealRow('warning', w.warning_id, `Warning: ${w.reason} · ${ago(w.secs_ago)}`, st.appeals));
  return out;
}

function bannedBar() {
  const b = securityInfo?.standing?.banned;
  if (!b || route.name === 'profile') return null;
  return el('div', { class: 'status error' }, `Your account is banned: ${b.reason}. `, el('a', { href: '/profile#standing' }, 'Appeal'));
}

function warningsBar() {
  const seen = async () => {
    const time = Date.now();
    const signature = signText(secretKey(), `libroxel warnings seen\n${session.playerId}\n${time}`);
    try {
      await api('/accounts/warnings/seen', { method: 'POST', body: { player_id: session.playerId, time, signature } });
      securityInfo.warnings = [];
    } catch (e) {
      flash = { text: e.message, kind: 'error' };
    }
    render();
  };
  return el('div', { class: 'status error' }, `Warning: ${securityInfo.warnings.map((w) => w.reason).join(' · ')}`,
    el('button', { class: 'chip', onclick: seen }, 'OK'), el('a', { href: '/profile#standing' }, 'Appeal'));
}

// --- Avatar -----------------------------------------------------------------

let draft = null;
let walletInfo = null;

function walletTime() {
  const time = Math.max(Date.now(), lastSigned.wallet + 1);
  lastSigned.wallet = time;
  return time;
}

function loadWallet() {
  if (!session || (walletInfo && walletInfo.for === session.playerId)) return;
  walletInfo = { for: session.playerId, loading: true };
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel wallet\n${session.playerId}\n${time}`);
  api(`/wallet/${session.playerId}`, { query: { time, signature } })
    .then((w) => { walletInfo = { for: session.playerId, ...w }; })
    .catch((e) => { walletInfo = { for: session.playerId, error: e.message }; })
    .finally(() => render());
}

async function walletPost(path, body, message) {
  const time = walletTime();
  const signature = signText(secretKey(), message(time));
  try {
    const w = await api(path, { method: 'POST', body: { player_id: session.playerId, ...body, time, signature } });
    if (w) walletInfo = { for: session.playerId, ...w };
    return true;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
    return false;
  } finally {
    render();
  }
}

const buyItem = (item) => walletPost('/shop/buy', { item_id: item.item_id, price: item.price },
  (t) => `libroxel buy\n${session.playerId}\n${item.item_id}\n${item.price}\n${t}`);

async function priceItem(item, price) {
  const ok = await walletPost(`/catalog/${encodeURIComponent(item.item_id)}/price`, { price },
    (t) => `libroxel price\n${session.playerId}\n${item.item_id}\n${price}\n${t}`);
  if (ok) { shop.key = null; shop.item = null; walletInfo = null; }
}

const limitedText = (l) => [
  'Limited',
  l.stock != null && (l.on_sale ? `${l.stock - l.sold} of ${l.stock} left` : 'Sold out'),
  l.stock == null && !l.on_sale && 'Sale over',
  l.on_sale && l.ends_in_secs != null && `${Math.ceil(l.ends_in_secs / 86400)}d left`,
  l.lowest != null && `from ${l.lowest}`,
].filter(Boolean).join(' · ');

const piecesText = (pieces) => pieces.map((p) => `${p.item_id}#${p.serial}`).join(',');

const trading = { mine: null, trades: null, sales: null, open: null, to: '', theirs: null, give: new Set(), get: new Set(), busy: false };

function loadLimited(force) {
  if (!session || (!force && trading.mine && trading.mine.for === session.playerId)) return;
  trading.mine = { for: session.playerId, list: trading.mine?.list || [] };
  api(`/accounts/${session.playerId}/limited`).then((list) => { trading.mine = { for: session.playerId, list }; render(); }).catch(() => {});
  const time = walletTime();
  const signature = signText(secretKey(), `libroxel trades\n${session.playerId}\n${time}`);
  api(`/trades/${session.playerId}`, { query: { time, signature } }).then((t) => { trading.trades = t; render(); }).catch(() => {});
}

function openSales(id) {
  trading.open = id;
  trading.sales = null;
  api(`/catalog/${encodeURIComponent(id)}/sales`).then((s) => { trading.sales = s; render(); })
    .catch((e) => { flash = { text: e.message, kind: 'error' }; render(); });
}

async function afterTrade(ok) {
  if (!ok) return;
  shop.key = null;
  shop.item = null;
  walletInfo = null;
  loadLimited(true);
  if (trading.open) openSales(trading.open);
}

async function makeLimited(item, stock, days) {
  const s = stock || null;
  const d = days || null;
  const time = walletTime();
  const signature = signText(secretKey(), `libroxel limited\n${session.playerId}\n${item.item_id}\n${s ?? ''}\n${d ?? ''}\n${time}`);
  try {
    await api(`/catalog/${encodeURIComponent(item.item_id)}/limited`, { method: 'POST', body: { player_id: session.playerId, stock: s, days: d, time, signature } });
    shop.key = null;
    shop.item = null;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
    render();
  }
}

const resale = (action, itemId, serial, price) => walletPost('/resale', { action, item_id: itemId, serial, price },
  (t) => `libroxel resale\n${session.playerId}\n${action}\n${itemId}\n${serial}\n${price}\n${t}`).then(afterTrade);

async function tradePost(action, { trade_id = 0, to_id = '', give = [], get = [] }) {
  const time = walletTime();
  const signature = signText(secretKey(), `libroxel trade\n${session.playerId}\n${action}\n${trade_id}\n${to_id}\n${piecesText(give)}\n${piecesText(get)}\n${time}`);
  try {
    await api('/trades', { method: 'POST', body: { player_id: session.playerId, action, trade_id, to_id, give, get, time, signature } });
    return true;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
    return false;
  } finally {
    render();
  }
}

async function findTrader() {
  trading.theirs = null;
  trading.get = new Set();
  try {
    const p = await api(`/accounts/${encodeURIComponent(trading.to.trim())}`);
    const list = await api(`/accounts/${p.player_id}/limited`);
    trading.theirs = { player_id: p.player_id, name: p.name, list };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

async function sendOffer() {
  const pick = (set) => [...set].map((k) => { const [item_id, serial] = k.split('\n'); return { item_id, serial: Number(serial) }; });
  const ok = await tradePost('offer', { to_id: trading.theirs.player_id, give: pick(trading.give), get: pick(trading.get) });
  if (ok) { trading.give = new Set(); trading.get = new Set(); trading.theirs = null; trading.to = ''; }
  afterTrade(ok);
}

function salesPanel(id) {
  if (trading.open !== id) openSales(id);
  const s = trading.sales;
  return [
    el('h2', { class: 'section' }, 'Resale'),
    !s ? el('p', { class: 'soft' }, 'Loading…') : [
      el('p', { class: 'soft' }, `${limitedText(s.limited)} · ${s.owners} owners · ${s.fee_percent}% fee`),
      s.listings.length ? s.listings.map((l) => el('div', { class: 'form-actions' }, `#${l.serial} by ${l.seller}`,
        l.seller_id === session.playerId ? el('span', { class: 'soft' }, String(l.price))
          : el('button', { class: 'chip go', onclick: () => resale('buy', s.item_id, l.serial, l.price) }, `Buy for ${l.price}`)))
        : el('p', { class: 'soft' }, 'None for sale.'),
      s.history.length > 0 && el('div', { class: 'caps' }, 'Price history'),
      s.history.map((h) => el('div', { class: 'soft' }, `${h.price} · #${h.serial} · ${ago(h.secs_ago)}`)),
    ],
  ];
}

function limitedPanel() {
  loadLimited();
  const mine = trading.mine?.list || [];
  const key = (o) => `${o.item_id}\n${o.serial}`;
  const tick = (set, o) => {
    const box = el('input', { type: 'checkbox', checked: set.has(key(o)) });
    box.addEventListener('change', () => { if (box.checked) set.add(key(o)); else set.delete(key(o)); render(); });
    return el('label', { class: 'chip' }, box, `${o.name} #${o.serial}`);
  };
  const owned = mine.map((o) => {
    const box = el('input', { type: 'number', min: 1, max: 100000, value: o.listed || '', style: 'width:80px' });
    return el('div', { class: 'form-actions' }, el('b', {}, `${o.name} #${o.serial}`),
      o.listed != null && el('span', { class: 'soft' }, `Listed for ${o.listed}`),
      box, el('button', { class: 'chip', onclick: () => resale('list', o.item_id, o.serial, Math.floor(Number(box.value) || 0)) }, 'Sell'),
      o.listed != null && el('button', { class: 'chip', onclick: () => resale('unlist', o.item_id, o.serial, 0) }, 'Unlist'));
  });
  const act = (t, action) => el('button', { class: 'chip', onclick: () => tradePost(action, { trade_id: t.trade_id }).then(afterTrade) },
    { accept: 'Accept', decline: 'Decline', cancel: 'Cancel', confirm: 'Confirm' }[action]);
  const trades = (trading.trades || []).map((t) => {
    const fromMe = t.from_id === session.playerId;
    const open = t.state === 'offered' || t.state === 'accepted';
    const mineOk = fromMe ? t.from_confirmed : t.to_confirmed;
    const list = (pieces) => pieces.map((p) => `${p.name} #${p.serial}`).join(', ') || 'nothing';
    return el('div', { class: 'item' },
      el('b', {}, fromMe ? `To ${t.to}` : `From ${t.from}`),
      el('small', {}, `${fromMe ? 'You give' : 'They give'}: ${list(t.give)}`),
      el('small', {}, `${fromMe ? 'You get' : 'They get'}: ${list(t.get)}`),
      el('small', {}, `${t.state}${t.state === 'accepted' ? ` · ${[t.from_confirmed, t.to_confirmed].filter(Boolean).length}/2 confirmed` : ''} · ${ago(t.secs_ago)}`),
      open && el('span', { class: 'form-actions' },
        t.state === 'offered' && !fromMe && [act(t, 'accept'), act(t, 'decline')],
        t.state === 'accepted' && !mineOk && act(t, 'confirm'),
        act(t, 'cancel')));
  });
  const to = el('input', { value: trading.to, placeholder: 'Name' });
  to.addEventListener('input', () => { trading.to = to.value; });
  const theirs = trading.theirs;
  return [
    el('h2', { class: 'section' }, 'Limited'),
    owned.length ? owned : el('p', { class: 'soft' }, 'No limited items yet.'),
    el('h2', { class: 'section' }, 'Trades'),
    el('div', { class: 'form-actions' }, to, el('button', { class: 'chip', onclick: findTrader }, 'Find')),
    theirs && [
      el('div', { class: 'caps' }, 'You give'),
      el('div', { class: 'form-actions' }, mine.map((o) => tick(trading.give, o))),
      el('div', { class: 'caps' }, `${theirs.name} gives`),
      el('div', { class: 'form-actions' }, theirs.list.map((o) => tick(trading.get, o))),
      el('div', { class: 'form-actions' }, el('button', { class: 'btn go small', disabled: !trading.give.size && !trading.get.size, onclick: sendOffer }, 'Send offer')),
    ],
    trades.length > 0 && el('div', { class: 'catalog' }, trades),
  ];
}

/** For now only Hat-Makers (the Hub's owners give it out, and are ones themselves) make catalog hats. */
const mayMakeItems = () => Boolean(data.me && (data.me.hat_maker || data.me.admin));

/** The account's Credits, at the top right by its name. */
function creditsChip() {
  loadWallet();
  const w = walletInfo;
  if (!w || w.loading || w.error || w.balance == null) return null;
  return el('a', { class: `tab credits${route.name === 'credits' ? ' active' : ''}`, href: '/credits', title: `Buy ${w.currency}` },
    icon(BADGE_GLYPHS.coin), String(w.balance));
}

function walletBar() {
  const w = walletInfo;
  if (!w || w.loading) return null;
  if (w.error) return status(w.error, 'error');
  return el('div', { class: 'form-actions' },
    el('b', {}, `${w.balance} ${w.currency}`),
    w.may_spend != null && el('span', { class: 'soft' }, w.may_spend ? `${w.may_spend} left to spend this month` : 'Spending is off'));
}

/**
 * The look and words being changed, from the account as the Hub has it.
 * Anything the saved look leaves out is the usual look for the name, as in
 * the app, in games and in the Hub's pictures. A look saved somewhere else
 * (the app, another computer) replaces one that hasn't been touched here.
 */
function startDraft() {
  const from = JSON.stringify(data.me.look ?? null);
  const mine = draft && draft.for === session.playerId;
  if (!mine || (!draft.touched && draft.from !== from)) {
    draft = {
      for: session.playerId, from, touched: false,
      look: readLook(data.me.look, data.me.name), about: data.me.about || '',
      message: mine ? draft.message : null,
    };
  }
}

const KIND_NAMES = { hat: 'Hat', shirt: 'Shirt', pants: 'Trousers' };
const SLOTS = { shirt: 'shirt_item', pants: 'pants_item' };

/** A picture of `look`, drawn by the Hub. */
function lookPicture(look, shot, size) {
  const json = encodeURIComponent(JSON.stringify(lookJson(look)));
  return el('img', { class: 'item-pic', src: `${HUB}/avatar/${shot}.png?name=item&look=${json}`, alt: '', loading: 'lazy', width: size, height: size });
}

/** An item on the usual look. */
function itemPicture(item, size = 120) {
  const look = lookFor(0n);
  if (item.kind === 'hat') {
    const hat = readItem(item.data);
    if (hat) look.item = hat;
  } else {
    const c = readClothing(item.data);
    if (c) look[SLOTS[item.kind]] = c;
  }
  return lookPicture(look, item.kind === 'hat' ? 'head' : 'body', size);
}

const hasItem = (item) => Boolean(session && (item.creator_id === session.playerId || walletInfo?.owned?.includes(item.item_id)));

function getButton(item) {
  const label = item.price ? `Buy for ${item.price}` : 'Get';
  if (!session) return el('a', { class: 'chip go', href: '/sign-in' }, label);
  if (hasItem(item)) return el('a', { class: 'chip', href: '/avatar' }, icon(ICON.check), 'Owned');
  if (item.limited && !item.limited.on_sale) return null;
  return el('button', { class: 'chip go', disabled: shop.busy, onclick: () => getItem(item) }, label);
}

async function getItem(item) {
  shop.busy = true;
  if (await buyItem(item)) {
    flash = { text: `${item.name} is in your inventory.` };
    if (item.limited) afterTrade(true);
  }
  shop.busy = false;
  render();
}

const AVATAR_TABS = [['body', 'Body'], ['hat', 'Hats'], ['shirt', 'Shirts'], ['pants', 'Trousers']];
let avatarTab = 'body';

function avatar() {
  if (!session) return needSignIn('Choosing how you look needs an account: sign in, or make one, and your avatar is the same in every game, here and in the app. Until then you play with the usual look.');
  if (!data.me) return [el('h1', { class: 'title' }, 'Your avatar'), el('p', { class: 'soft' }, data.loaded ? 'Your account could not be loaded.' : 'Loading…')];
  startDraft();
  loadWallet();
  const look = draft.look;
  const changed = () => { draft.message = null; draft.touched = true; render(); };
  const swatches = (key, choices) => {
    const pick = el('input', { type: 'color', class: 'swatch pick', value: `#${look[key].map((v) => v.toString(16).padStart(2, '0')).join('')}` });
    pick.addEventListener('change', () => { look[key] = pick.value.match(/[0-9a-f]{2}/gi).map((h) => parseInt(h, 16)); changed(); });
    return el('div', { class: 'swatches' }, choices.map((c) => el('button', {
      class: `swatch${look[key].join() === c.join() ? ' on' : ''}`, style: `--c:${c.join(', ')}`, title: rgb(c),
      onclick: () => { look[key] = c; changed(); },
    })), pick);
  };
  const tile = (on, picture, label, onclick) => el('button', { class: `own${on ? ' on' : ''}`, onclick }, picture, el('span', {}, label));
  const owned = (walletInfo?.items || []).filter((i) => i.kind === avatarTab);
  const wear = (item) => api(`/catalog/${encodeURIComponent(item.item_id)}/wear`, { method: 'POST' }).catch(() => {});
  const none = el('p', { class: 'soft' }, 'Nothing here yet. ', el('a', { href: '/market' }, 'Marketplace'));
  let panel;
  if (avatarTab === 'body') {
    panel = [
      el('div', { class: 'caps' }, 'Body'),
      el('div', { class: 'hats' }, BODIES.map((b) => el('button', { class: `chip${(look.body || 'Classic') === b ? ' on' : ''}`, onclick: () => { look.body = b; changed(); } }, b))),
      el('div', { class: 'caps' }, 'Skin'), swatches('skin', AVATAR.skins),
    ];
  } else if (avatarTab === 'hat') {
    const hats = owned.map((item) => [item, readItem(item.data)]).filter(([, hat]) => hat);
    const same = (hat) => look.item && JSON.stringify(look.item.json) === JSON.stringify(hat.json);
    panel = [
      el('div', { class: 'owns' },
        HATS.map((h) => tile(look.hat === h && !look.item, lookPicture({ ...lookFor(0n), hat: h }, 'head', 84), h, () => { look.hat = h; delete look.item; changed(); })),
        hats.map(([item, hat]) => tile(same(hat), itemPicture(item, 84), item.name, () => { look.item = hat; wear(item); changed(); }))),
      look.hat !== 'No hat' && !look.item && [el('div', { class: 'caps' }, 'Hat colour'), swatches('hat_color', AVATAR.clothes)],
      !hats.length && none,
    ];
  } else {
    const slot = SLOTS[avatarTab];
    const clothes = owned.map((item) => [item, readClothing(item.data)]).filter(([, c]) => c);
    const same = (c) => JSON.stringify(look[slot] || null) === JSON.stringify(c);
    panel = [
      el('div', { class: 'caps' }, 'Colour'), swatches(avatarTab, AVATAR.clothes),
      el('div', { class: 'owns' },
        tile(!look[slot], lookPicture({ ...look, [slot]: undefined, item: undefined, hat: 'No hat' }, 'body', 84), 'None', () => { delete look[slot]; changed(); }),
        clothes.map(([item, c]) => tile(same(c), itemPicture(item, 84), item.name, () => { look[slot] = c; wear(item); changed(); }))),
      !clothes.length && none,
    ];
  }
  const save = el('button', { class: 'btn go', onclick: () => saveAccount(save) }, icon(ICON.check), 'Save');
  return [
    el('h1', { class: 'title' }, 'Your avatar'),
    el('div', { class: 'avatar-page' },
      el('div', { class: 'figure' }, figure(look, data.me.name)),
      el('div', { style: 'flex:1;min-width:280px' },
        el('div', { class: 'hats' }, AVATAR_TABS.map(([t, label]) => el('button', { class: `chip${avatarTab === t ? ' on' : ''}`, onclick: () => { avatarTab = t; render(); } }, label))),
        walletInfo?.loading ? el('p', { class: 'soft' }, 'Loading…') : panel,
        el('div', { class: 'form-actions' },
          save,
          el('button', { class: 'chip', onclick: () => { draft.look = lookFor(0n); changed(); } }, 'Back to the usual look')),
        draft.message && status(draft.message.text, draft.message.kind))),
  ];
}

const clothesForm = { name: '', kind: 'shirt', color: '#2060c0', sleeves: false, busy: false };

/** Shares a shirt or trousers to the catalog (Hat-Makers only, as hats). */
async function shareClothes() {
  const f = clothesForm;
  const name = f.name.trim();
  if (!name || f.busy) return;
  f.busy = true;
  render();
  const data = { color: f.color.match(/[0-9a-f]{2}/gi).map((h) => parseInt(h, 16)), name };
  if (f.kind === 'shirt') data.sleeves = f.sleeves;
  // Keys in the order the Hub writes them, as the signature covers the text.
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel item\n${session.playerId}\n${name}\n${f.kind}\n${JSON.stringify(data)}\n${time}`);
  try {
    await api('/catalog', { method: 'POST', body: { creator_id: session.playerId, name, kind: f.kind, data, time, signature } });
    flash = { text: `${name} is in the Marketplace.` };
    f.name = '';
    shop.key = null;
    walletInfo = null;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  f.busy = false;
  render();
}

function clothesMaker() {
  const f = clothesForm;
  const kind = pickFrom({ shirt: 'Shirt', pants: 'Trousers' }, f.kind);
  kind.addEventListener('change', () => { f.kind = kind.value; render(); });
  const colour = el('input', { type: 'color', class: 'swatch pick', value: f.color });
  colour.addEventListener('change', () => { f.color = colour.value; });
  const sleeves = el('input', { type: 'checkbox', checked: f.sleeves });
  sleeves.addEventListener('change', () => { f.sleeves = sleeves.checked; });
  return el('div', { class: 'form manage' },
    el('div', { class: 'caps' }, 'Make clothes'),
    el('label', { class: 'field' }, el('span', {}, 'Name'), el('input', { type: 'text', maxlength: 40, value: f.name, oninput: (e) => { f.name = e.target.value; } })),
    el('label', { class: 'field' }, el('span', {}, 'Kind'), kind),
    el('label', { class: 'field' }, el('span', {}, 'Colour'), colour),
    f.kind === 'shirt' && el('label', { class: 'field' }, el('span', {}, 'Long sleeves'), sleeves),
    el('div', { class: 'form-actions' }, el('button', { class: 'btn go small', disabled: f.busy, onclick: shareClothes }, icon(ICON.check), 'Add to the Marketplace')));
}

function profile() {
  if (!session) return needSignIn('Sign in to see your profile.');
  if (!data.me && securityInfo?.standing?.banned) return [el('h1', { class: 'title' }, 'Profile'), standingPanel(), el('div', { class: 'form-actions', style: 'margin-top:18px' }, el('button', { class: 'chip', onclick: signOut }, icon(ICON.signOut), 'Sign out'))];
  if (!data.me) return [el('h1', { class: 'title' }, 'Profile'), el('p', { class: 'soft' }, data.loaded ? 'Your account could not be loaded.' : 'Loading…')];
  startDraft();
  const about = el('textarea', { rows: 3, maxlength: 2000 }, draft.about);
  about.addEventListener('input', () => { draft.about = about.value; draft.touched = true; });
  const save = el('button', { class: 'btn go', onclick: () => saveAccount(save) }, icon(ICON.check), 'Save');
  return [
    el('div', { class: 'hello' },
      portrait(session.playerId, 84, data.me.look),
      el('div', {},
        el('h1', {}, data.me.name),
        el('div', { class: 'soft' }, `#${data.me.number ?? 0} · `, el('a', { href: `/user/${data.me.number ?? session.playerId}` }, 'Public page'), ' · ', el('a', { href: '/friends' }, 'Friends')))),
    el('label', { class: 'field' }, el('span', {}, 'About you'), about),
    el('div', { class: 'form-actions' }, save),
    draft.message && status(draft.message.text, draft.message.kind),
    security(),
    standingPanel(),
    parentPanel(),
    el('div', { class: 'form-actions', style: 'margin-top:18px' }, el('button', { class: 'chip', onclick: signOut }, icon(ICON.signOut), 'Sign out')),
  ];
}

async function saveAccount(button) {
  button.disabled = true;
  const look = lookJson(draft.look);
  const name = data.me.name;
  const about = draft.about;
  const time = Math.max(nowSecs(), lastSigned.account + 1);
  lastSigned.account = time;
  const signature = signText(secretKey(), `libroxel account\n${session.playerId}\n${name}\n${about}\n${canonical(look)}\n${time}`);
  try {
    await api('/accounts', { method: 'POST', body: { player_id: session.playerId, name, about, look, time, signature } });
    draft.message = { text: 'Saved. You look like this in every game now.' };
    await refresh();
    Object.assign(draft, { look: readLook(data.me.look, name), from: JSON.stringify(data.me.look ?? null), touched: false });
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
  (async () => {
    if (!session.session) {
      const time = Date.now();
      const device = deviceName();
      const signature = signText(secretKey(), `libroxel session\n${session.playerId}\n${device}\n${time}`);
      const started = await api('/accounts/session', { method: 'POST', body: { player_id: session.playerId, device, time, signature } }).catch(() => null);
      if (started) saveSession({ ...session, session: started.session }, isRemembered());
    }
    const time = Date.now();
    const signature = signText(secretKey(), `libroxel security\n${session.playerId}\n${time}`);
    return api(`/accounts/${session.playerId}/security`, { query: { time, signature, session: session.session || '' } });
  })()
    .then((s) => { securityInfo = { for: session.playerId, ...s }; render(); })
    .catch((e) => {
      if (e.status === 410) return signedOut();
      securityInfo = { for: session.playerId, error: e.message };
      render();
    });
}

const twoStep = { secret: null, codes: null, message: null, busy: false };

async function changeTwoStep(secret, code) {
  const time = nowSecs();
  const signature = signText(secretKey(), `libroxel two-step\n${session.playerId}\n${secret || ''}\n${code}\n${time}`);
  twoStep.busy = true;
  render();
  try {
    const done = await api('/accounts/two-step', { method: 'POST', body: { player_id: session.playerId, secret, code, time, signature } });
    twoStep.secret = null;
    twoStep.codes = done.codes.length ? done.codes : null;
    twoStep.message = { text: secret ? 'Two-step sign-in is on.' : 'Two-step sign-in is off.' };
    securityInfo = null;
  } catch (e) {
    twoStep.message = { text: e.message, kind: 'error' };
  }
  twoStep.busy = false;
  render();
}

async function newRecoveryCodes() {
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel recovery codes\n${session.playerId}\n${time}`);
  try {
    twoStep.codes = (await api('/accounts/recovery-codes', { method: 'POST', body: { player_id: session.playerId, time, signature } })).codes;
    securityInfo = null;
  } catch (e) {
    twoStep.message = { text: e.message, kind: 'error' };
  }
  render();
}

function twoStepPanel() {
  const s = securityInfo;
  const out = [el('div', { class: 'caps', style: 'margin-top:12px' }, 'Two-step sign-in')];
  const code = () => el('input', { type: 'text', inputmode: 'numeric', autocomplete: 'one-time-code', maxlength: 12, placeholder: '123456', style: 'max-width:140px' });
  if (!s.has_password) out.push(el('p', { class: 'soft' }, 'Set a password first.'));
  else if (s.two_step) {
    const box = code();
    out.push(el('p', { style: 'color:var(--green)' }, icon(ICON.check), ' On'));
    out.push(el('div', { class: 'search' }, box,
      el('button', { class: 'chip', disabled: twoStep.busy, onclick: () => changeTwoStep(null, box.value.trim()) }, 'Turn off')));
    out.push(el('p', { class: 'soft' }, `Recovery codes left: ${s.recovery_codes_left} `,
      el('button', { class: 'link', type: 'button', onclick: newRecoveryCodes }, 'New codes')));
  } else if (twoStep.secret) {
    const link = `otpauth://totp/PlayLibre:${encodeURIComponent(session.name)}?secret=${twoStep.secret}&issuer=PlayLibre&digits=6&period=30`;
    const box = code();
    out.push(el('p', {}, el('code', {}, twoStep.secret), ' ',
      el('button', { class: 'link', type: 'button', onclick: () => navigator.clipboard?.writeText(twoStep.secret) }, 'Copy'), ' ',
      el('a', { href: link }, 'Open in app')));
    out.push(el('div', { class: 'search' }, box,
      el('button', { class: 'btn go small', disabled: twoStep.busy, onclick: () => changeTwoStep(twoStep.secret, box.value.trim()) }, 'Turn on'),
      el('button', { class: 'chip', onclick: () => { twoStep.secret = null; render(); } }, 'Cancel')));
  } else {
    out.push(el('button', { class: 'chip', onclick: () => { twoStep.secret = base32(random(20)); render(); } }, 'Set up'));
  }
  if (twoStep.codes) {
    out.push(el('pre', { class: 'codes' }, twoStep.codes.join('\n')));
    out.push(el('div', { class: 'search' },
      el('button', { class: 'chip', onclick: () => navigator.clipboard?.writeText(twoStep.codes.join('\n')) }, 'Copy'),
      el('button', { class: 'chip', onclick: () => { twoStep.codes = null; render(); } }, 'Done')));
  }
  if (twoStep.message) {
    out.push(status(twoStep.message.text, twoStep.message.kind || ''));
    twoStep.message = null;
  }
  return out;
}

let devicesMessage = null;

function devicesPanel() {
  const s = securityInfo;
  const out = [el('div', { class: 'caps', style: 'margin-top:12px' }, 'Devices'),
    el('ul', { class: 'signins' }, (s.sessions || []).map((d) => el('li', {}, `${d.device}${d.this ? ' (this one)' : ''}, ${d.from}, ${ago(d.seen_secs_ago)}`)))];
  if (s.has_password) {
    const password = el('input', { type: 'password', autocomplete: 'current-password', placeholder: 'Password', style: 'max-width:180px' });
    const code = s.two_step && el('input', { type: 'text', inputmode: 'numeric', autocomplete: 'one-time-code', maxlength: 12, placeholder: 'Code', style: 'max-width:110px' });
    const go = el('button', { class: 'chip' }, icon(ICON.signOut), 'Sign out everywhere');
    go.onclick = async () => {
      go.disabled = true;
      try {
        await signOutEverywhere(password.value, code?.value.trim());
        devicesMessage = { text: 'Signed out everywhere else.' };
        securityInfo = null;
      } catch (e) {
        devicesMessage = { text: e.message, kind: 'error' };
      }
      render();
    };
    out.push(el('div', { class: 'search' }, password, code, go));
  }
  if (devicesMessage) {
    out.push(status(devicesMessage.text, devicesMessage.kind || ''));
    devicesMessage = null;
  }
  return out;
}

/** Whether `email` looks like an address (the Hub checks properly). */
function validEmail(email) {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim()) && email.trim().length <= 254;
}

/** A note at the top of every page while your account has no proved email. */
function emailNudge() {
  if (!session || !securityInfo || securityInfo.loading || securityInfo.error || route.name === 'profile') return null;
  if (!securityInfo.email) return el('div', { class: 'status wait' }, 'Your account has no email yet. ', el('a', { href: '/profile' }, 'Add one'), ' so you can prove the account is yours.');
  if (!securityInfo.email_verified && securityInfo.sends_mail) return el('div', { class: 'status wait' }, `Prove your email: type the code we mailed to ${securityInfo.email} `, el('a', { href: '/profile' }, 'on your profile'), '.');
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
      el('a', { href: '/password' }, securityInfo.has_password ? 'Change your password' : 'Set a password'), '.',
    ),
    ...emailPanel(),
    ...twoStepPanel(),
    ...devicesPanel(),
  ]);
  return panel;
}

// --- Create -----------------------------------------------------------------

const creator = { days: 7, result: null, loading: false, error: null, for: null };

function loadCreator() {
  const key = `${session.playerId}/${creator.days}`;
  if (creator.loading || creator.for === key) return;
  creator.loading = true;
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel creator\n${session.playerId}\n${time}`);
  api(`/creator/${session.playerId}`, { query: { time, signature, days: creator.days } })
    .then((r) => { creator.result = r; creator.error = null; })
    .catch((e) => { creator.error = e.message; })
    .finally(() => { creator.loading = false; creator.for = key; if (route.name === 'creator') render(); });
}

const mine = { list: null, loading: false, error: null, for: null, versions: {}, busy: false };

function loadMine(again) {
  if (again) mine.for = null;
  if (mine.loading || mine.for === session.playerId) return;
  mine.loading = true;
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel creator\n${session.playerId}\n${time}`);
  api(`/creator/${session.playerId}/games`, { query: { time, signature } })
    .then((r) => { mine.list = r; mine.error = null; })
    .catch((e) => { mine.error = e.message; })
    .finally(() => { mine.loading = false; mine.for = session.playerId; if (route.name === 'creator') render(); });
}

function loadVersions(id, again) {
  if (!again && mine.versions[id]) return;
  mine.versions[id] = { loading: true };
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel creator\n${session.playerId}\n${time}`);
  api(`/games/${encodeURIComponent(id)}/versions`, { query: { player_id: session.playerId, time, signature } })
    .then((list) => { mine.versions[id] = { list }; })
    .catch((e) => { mine.versions[id] = { error: e.message }; })
    .finally(() => { if (route.name === 'creator') render(); });
}

async function manageGame(id, change, said, then) {
  if (mine.busy) return;
  mine.busy = true;
  render();
  const time = Date.now();
  const what = change.do === 'settings'
    ? `settings\n${change.private ? 1 : 0}\n${change.max_players}\n${change.title}\n${change.description}`
    : change.do === 'details' ? `details\n${change.genre}\n${change.tags.join(',')}`
    : change.do === 'restore' ? `restore\n${change.version}` : 'remove';
  const signature = signText(secretKey(), `libroxel manage\n${session.playerId}\n${id}\n${time}\n${what}`);
  try {
    await api(`/games/${encodeURIComponent(id)}/manage`, { method: 'POST', body: { player_id: session.playerId, time, signature, change } });
    flash = { text: said };
    loadMine(true);
    creator.for = null;
    delete mine.versions[id];
    if (then) then();
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  } finally {
    mine.busy = false;
    render();
  }
}

function settingsTab(g) {
  const title = el('input', { type: 'text', value: g.title, maxlength: 120 });
  const about = el('textarea', { rows: 4, maxlength: 2000 }, g.description);
  const privacy = pickFrom({ public: 'Public', private: 'Private' }, g.private ? 'private' : 'public');
  const most = el('input', { type: 'number', min: 0, max: 100, value: g.max_players || '', placeholder: 'Any' });
  const save = () => manageGame(g.game_id, {
    do: 'settings', title: title.value.trim(), description: about.value.trim(), private: privacy.value === 'private',
    max_players: Math.max(0, Math.min(100, Math.floor(Number(most.value) || 0))),
  }, 'Saved.');
  const remove = el('button', { class: 'chip', disabled: mine.busy, onclick: (e) => {
    const b = e.currentTarget;
    if (b.dataset.sure) manageGame(g.game_id, { do: 'remove' }, `Removed ${g.title}.`, () => goTo('/creator'));
    else { b.dataset.sure = '1'; b.replaceChildren(icon(ICON.trash), ' Remove?'); }
  } }, icon(ICON.trash), 'Remove');
  const genres = { '': 'None' };
  for (const g of GENRES) genres[g] = g;
  const genre = pickFrom(genres, g.genre || '');
  const tags = el('input', { type: 'text', value: (g.tags || []).join(', '), maxlength: 120, placeholder: 'Up to 5, such as hard, tower, co-op' });
  const saveDetails = () => manageGame(g.game_id, {
    do: 'details', genre: genre.value,
    tags: tags.value.split(',').map((t) => t.trim().toLowerCase().replace(/\s+/g, ' ')).filter(Boolean),
  }, 'Saved.');
  return el('div', {},
    el('div', { class: 'form manage' },
      el('label', { class: 'field' }, el('span', {}, 'Name'), title),
      el('label', { class: 'field' }, el('span', {}, 'Description'), about),
      el('label', { class: 'field' }, el('span', {}, 'Who can play'), privacy),
      el('label', { class: 'field' }, el('span', {}, 'Max players'), most),
      el('div', { class: 'form-actions' },
        el('button', { class: 'btn go small', disabled: mine.busy, onclick: save }, icon(ICON.check), 'Save'),
        remove)),
    el('h2', { class: 'subhead' }, 'Genre and tags'),
    el('div', { class: 'form manage' },
      el('label', { class: 'field' }, el('span', {}, 'Genre'), genre),
      el('label', { class: 'field' }, el('span', {}, 'Tags'), tags),
      el('div', { class: 'form-actions' },
        el('button', { class: 'btn go small', disabled: mine.busy, onclick: saveDetails }, icon(ICON.check), 'Save'))),
    picturesPanel(g),
    placesPanel(g));
}

// --- Places: a game's other places, which TeleportService sends players to ---

const placeLists = new Map();

function loadPlaces(gameId) {
  if (placeLists.has(gameId)) return placeLists.get(gameId);
  placeLists.set(gameId, null);
  api(`/games/${encodeURIComponent(gameId)}/places`).then((list) => { placeLists.set(gameId, list); render(); }).catch(() => placeLists.set(gameId, []));
  return null;
}

async function changePlace(g, placeId, name, text, said) {
  if (mine.busy) return;
  mine.busy = true;
  render();
  const time = Math.max(Date.now(), lastSigned.place + 1);
  lastSigned.place = time;
  const what = text == null ? 'remove' : await sha256Hex(new TextEncoder().encode(text));
  const body = { player_id: session.playerId, time, place_id: placeId, name, place: text, signature: '' };
  body.signature = signText(secretKey(), `playlibre place\n${session.playerId}\n${g.game_id}\n${time}\n${placeId}\n${name}\n${what}`);
  try {
    placeLists.set(g.game_id, await api(`/games/${encodeURIComponent(g.game_id)}/places`, { method: 'POST', body }));
    flash = { text: said };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  mine.busy = false;
  render();
}

function placesPanel(g) {
  const list = loadPlaces(g.game_id);
  const name = el('input', { type: 'text', maxlength: 50, placeholder: 'Such as Level 2' });
  const file = el('input', { type: 'file', accept: '.lrx' });
  const add = async () => {
    const f = file.files[0];
    if (!name.value.trim() || !f) { flash = { text: 'Give the place a name and pick its .lrx file.', kind: 'error' }; render(); return; }
    changePlace(g, 0, name.value.trim(), await f.text(), `Added ${name.value.trim()}.`);
  };
  const replace = (p) => {
    const input = el('input', { type: 'file', accept: '.lrx', style: 'display:none' });
    input.addEventListener('change', async () => input.files[0] && changePlace(g, p.place_id, p.name, await input.files[0].text(), `Updated ${p.name}.`));
    return el('label', { class: 'chip', disabled: mine.busy }, input, icon(ICON.refresh), 'Update');
  };
  return el('div', {},
    el('h2', { class: 'subhead' }, 'Places'),
    el('p', { class: 'note' }, 'More places for this game, such as a lobby and its levels. Scripts send players between them with TeleportService and the place number, which game.PlaceId gives in each place. Saved data is shared by all of them.'),
    !list ? el('p', { class: 'soft' }, 'Loading…') : el('div', { class: 'versions' }, list.map((p) => el('div', { class: 'person' },
      el('div', { class: 'what' }, el('b', {}, p.start ? `${p.name} (start)` : p.name), el('small', { class: 'soft' }, `Place ${p.place_id} · v${p.version} · ${size(p.size)}`)),
      el('div', { class: 'buttons' }, p.start
        ? el('span', { class: 'soft' }, 'Changes when you publish')
        : [replace(p), el('button', { class: 'chip', disabled: mine.busy, onclick: () => changePlace(g, p.place_id, p.name, null, `Removed ${p.name}.`) }, icon(ICON.trash), 'Remove')])))),
    el('div', { class: 'form manage' },
      el('label', { class: 'field' }, el('span', {}, 'Name'), name),
      el('label', { class: 'field' }, el('span', {}, 'Place file'), file),
      el('div', { class: 'form-actions' }, el('button', { class: 'btn go small', disabled: mine.busy, onclick: add }, icon(ICON.check), 'Add place'))));
}

// The game genres a creator can pick (the Hub's GENRES).
const GENRES = ['Racing', 'Fighting', 'Obby', 'Board and cards', 'Party', 'Tycoon and sim', 'Adventure', 'Horror', 'Shooter', 'Sports', 'Building'];
const MAX_SCREENSHOTS = 6;

async function sha256Hex(bytes) {
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return Array.from(hash, (b) => b.toString(16).padStart(2, '0')).join('');
}

function base64Of(bytes) {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

/** Puts up (a File) or takes down (null) a game's icon (slot 0) or a screenshot. */
async function changePicture(g, slot, file) {
  if (mine.busy) return;
  mine.busy = true;
  render();
  try {
    const bytes = file ? new Uint8Array(await file.arrayBuffer()) : null;
    if (bytes && bytes.length > 6 * 1024 * 1024) throw new Error('Pictures can be at most 6 MB.');
    const time = Math.max(Date.now(), lastSigned.media + 1);
    lastSigned.media = time;
    const what = bytes ? await sha256Hex(bytes) : 'remove';
    const signature = signText(secretKey(), `playlibre media\n${session.playerId}\n${g.game_id}\n${time}\n${slot}\n${what}`);
    g.media = await api(`/games/${encodeURIComponent(g.game_id)}/media`, {
      method: 'POST', body: { player_id: session.playerId, time, slot, image: bytes ? base64Of(bytes) : null, signature },
    });
    flash = { text: file ? 'Picture saved.' : 'Picture taken down.' };
    loadMine(true);
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  } finally {
    mine.busy = false;
    render();
  }
}

function picturesPanel(g) {
  const media = g.media || [];
  const picker = (slot, label) => {
    const input = el('input', { type: 'file', accept: 'image/png,image/jpeg', style: 'display:none' });
    input.addEventListener('change', () => input.files[0] && changePicture(g, slot, input.files[0]));
    return el('label', { class: 'chip', disabled: mine.busy }, input, label);
  };
  const tile = (m, label) => el('div', { class: 'shot-tile' },
    el('img', { src: mediaUrl(g.game_id, m), alt: label }),
    el('button', { class: 'chip', disabled: mine.busy, onclick: () => changePicture(g, m.slot, null) }, icon(ICON.trash), 'Take down'));
  const icon0 = media.find((m) => m.slot === 0);
  const shots = media.filter((m) => m.slot > 0);
  const free = [];
  for (let slot = 1; slot <= MAX_SCREENSHOTS; slot++) if (!shots.some((m) => m.slot === slot)) free.push(slot);
  return el('div', {},
    el('h2', { class: 'subhead' }, 'Pictures'),
    el('p', { class: 'note' }, `PNG or JPEG. The icon is cut to a square; screenshots are shown up to ${MAX_SCREENSHOTS} at a time.`),
    el('div', { class: 'caps group' }, 'Icon'),
    el('div', { class: 'shot-row' }, icon0 ? tile(icon0, 'Icon') : null, picker(0, icon0 ? 'Change icon' : 'Add an icon')),
    el('div', { class: 'caps group' }, 'Screenshots'),
    el('div', { class: 'shot-row' }, shots.map((m) => tile(m, 'Screenshot')), free.length ? picker(free[0], 'Add a screenshot') : null));
}

function versionsTab(g) {
  loadVersions(g.game_id);
  const v = mine.versions[g.game_id];
  if (!v || v.loading) return el('p', { class: 'soft' }, 'Loading…');
  if (v.error) return status(v.error, 'error');
  return el('div', { class: 'versions' }, v.list.map((x) => el('div', { class: 'person' },
    el('div', { class: 'what' }, el('b', {}, `Version ${x.version}`), el('small', { class: 'soft' }, `${ago(x.published_secs_ago)} · ${size(x.size)}`)),
    el('div', { class: 'buttons' }, x.version === g.version
      ? el('span', { class: 'soft' }, 'Live')
      : el('button', { class: 'chip', disabled: mine.busy, onclick: () => manageGame(g.game_id, { do: 'restore', version: x.version }, `Version ${x.version} is live again.`) }, icon(ICON.refresh), 'Restore')))));
}

const saved = { game: null, store: '', prefix: '', view: null, error: null, loading: false, editing: null, text: '', adding: false, busy: false };

function loadSaved(id, again) {
  if (saved.loading || (!again && saved.game === id && saved.view)) return;
  if (saved.game !== id) Object.assign(saved, { game: id, store: '', prefix: '', view: null, error: null, editing: null, adding: false });
  saved.loading = true;
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel creator\n${session.playerId}\n${time}`);
  const query = { player_id: session.playerId, time, signature };
  if (saved.store) query.store = saved.store;
  if (saved.store && saved.prefix) query.prefix = saved.prefix;
  api(`/games/${encodeURIComponent(id)}/data`, { query })
    .then((v) => {
      const text = JSON.stringify(v);
      const changed = text !== saved.shown || saved.error;
      saved.shown = text;
      saved.view = v;
      saved.error = null;
      if (changed && route.name === 'creator' && route.tab === 'data') render();
    })
    .catch((e) => { saved.error = e.message; if (route.name === 'creator') render(); })
    .finally(() => { saved.loading = false; });
}

async function editSaved(id, store, key, value, said) {
  if (saved.busy) return;
  saved.busy = true;
  render();
  const time = Math.max(Date.now(), lastSigned.data + 1);
  lastSigned.data = time;
  const what = value === null ? 'remove' : `set\n${value}`;
  const signature = signText(secretKey(), `libroxel data\n${session.playerId}\n${id}\n${time}\n${store}\n${key}\n${what}`);
  try {
    await api(`/games/${encodeURIComponent(id)}/data/edit`, { method: 'POST', body: { player_id: session.playerId, time, store, key, value, signature } });
    flash = { text: said };
    Object.assign(saved, { editing: null, adding: false });
    loadSaved(id, true);
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  } finally {
    saved.busy = false;
    render();
  }
}

function dataTab(id) {
  loadSaved(id);
  if (saved.error && !saved.view) return status(saved.error, 'error');
  const v = saved.view;
  if (!v) return el('p', { class: 'soft' }, 'Loading…');
  const out = [el('p', { class: 'soft' }, `${size(v.used)} of ${size(v.limit)} used`)];
  if (!v.stores.length && !saved.store) return [...out, el('div', { class: 'empty' }, 'No saved data yet.')];
  const pickStore = (name) => { Object.assign(saved, { store: name, prefix: '', editing: null, adding: false }); loadSaved(id, true); render(); };
  out.push(el('div', { class: 'hats' }, v.stores.map((st) =>
    el('button', { class: `chip${saved.store === st.name ? ' on' : ''}`, onclick: () => pickStore(st.name) }, `${st.name} (${count(st.keys)})`))));
  if (!saved.store) return out;
  const find = el('input', { type: 'search', placeholder: 'Key starts with', value: saved.prefix });
  find.addEventListener('keydown', (e) => { if (e.key === 'Enter') { saved.prefix = find.value; loadSaved(id, true); } });
  out.push(el('div', { class: 'search' }, find,
    el('button', { class: 'chip', onclick: () => { saved.prefix = find.value; loadSaved(id, true); } }, icon(ICON.search), 'Find'),
    el('button', { class: 'chip', onclick: () => { Object.assign(saved, { adding: true, editing: null, text: '' }); render(); } }, 'Add key')));
  const editor = (key, isNew) => {
    const name = isNew && el('input', { type: 'text', placeholder: 'Key', maxlength: 200 });
    const box = el('textarea', { rows: 6, class: 'mono', oninput: (e) => { saved.text = e.target.value; } }, saved.text);
    const save = () => {
      const k = isNew ? name.value.trim() : key;
      if (!k) return;
      try { JSON.parse(box.value); } catch { flash = { text: "That isn't valid JSON.", kind: 'error' }; render(); return; }
      editSaved(id, saved.store, k, box.value, 'Saved.');
    };
    return el('div', { class: 'form manage' }, name, box,
      el('div', { class: 'form-actions' },
        el('button', { class: 'btn go small', disabled: saved.busy, onclick: save }, icon(ICON.check), 'Save'),
        el('button', { class: 'chip', onclick: () => { Object.assign(saved, { editing: null, adding: false }); render(); } }, 'Cancel')));
  };
  if (saved.adding) out.push(editor('', true));
  const rows = v.keys.map((k) => {
    const shown = k.value === null || k.value === undefined ? `Too big to show (${size(k.size)})` : JSON.stringify(k.value);
    const row = el('div', { class: 'person' },
      el('div', { class: 'what' }, el('b', {}, k.key), el('small', { class: 'soft mono' }, shown)),
      el('div', { class: 'buttons' },
        k.value !== null && k.value !== undefined && el('button', { class: 'chip', disabled: saved.busy, onclick: () => { Object.assign(saved, { editing: k.key, adding: false, text: JSON.stringify(k.value, null, 2) }); render(); } }, 'Edit'),
        el('button', { class: 'chip', disabled: saved.busy, onclick: (e) => {
          const btn = e.currentTarget;
          if (btn.dataset.sure) editSaved(id, saved.store, k.key, null, `Removed ${k.key}.`);
          else { btn.dataset.sure = '1'; btn.replaceChildren(icon(ICON.trash), ' Remove?'); }
        } }, icon(ICON.trash), 'Remove')));
    return saved.editing === k.key ? [row, editor(k.key, false)] : row;
  });
  out.push(rows.length ? el('div', { class: 'versions' }, rows) : el('p', { class: 'soft' }, 'No keys.'));
  if (v.more) out.push(el('p', { class: 'soft' }, `Showing the first ${v.keys.length}. Narrow it down with a key.`));
  return out;
}

const badgeForm = { name: '', description: '', icon: 'star', color: 'yellow', editing: null };

async function changeBadge(gameId, action, b, said) {
  if (mine.busy) return;
  mine.busy = true;
  render();
  const time = Math.max(Date.now(), lastSigned.badge + 1);
  lastSigned.badge = time;
  const c = { player_id: session.playerId, action, badge_id: b.badge_id || '', name: b.name || '', description: b.description || '', icon: b.icon || '', color: b.color || '', time };
  c.signature = signText(secretKey(), `libroxel badge\n${c.player_id}\n${gameId}\n${c.action}\n${c.badge_id}\n${c.name}\n${c.description}\n${c.icon}\n${c.color}\n${c.time}`);
  try {
    const list = await api(`/games/${encodeURIComponent(gameId)}/badges`, { method: 'POST', body: c });
    badgeLists.set(`/games/${encodeURIComponent(gameId)}/badges`, list);
    badgeLists.delete(`/accounts/${session.playerId}/badges`);
    Object.assign(badgeForm, { name: '', description: '', editing: null });
    flash = { text: said };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  } finally {
    mine.busy = false;
    render();
  }
}

function badgesTab(g) {
  const list = loadBadges(`/games/${encodeURIComponent(g.game_id)}/badges`);
  if (!list) return el('p', { class: 'soft' }, 'Loading…');
  const f = badgeForm;
  const name = el('input', { type: 'text', value: f.name, maxlength: 40, oninput: (e) => { f.name = e.target.value; } });
  const about = el('input', { type: 'text', value: f.description, maxlength: 200, oninput: (e) => { f.description = e.target.value; } });
  const color = colorPick(f.color);
  color.addEventListener('change', () => { f.color = color.value; render(); });
  const icons = el('div', { class: 'badge-icons' }, Object.keys(BADGE_GLYPHS).map((i) =>
    el('button', { class: `chip${f.icon === i ? ' on' : ''}`, title: i, onclick: () => { f.icon = i; render(); } }, icon(BADGE_GLYPHS[i]))));
  const save = () => {
    const b = { badge_id: f.editing || '', name: name.value.trim(), description: about.value.trim(), icon: f.icon, color: f.color };
    changeBadge(g.game_id, f.editing ? 'update' : 'create', b, f.editing ? 'Saved.' : `Made ${b.name}.`);
  };
  const rows = list.filter((b) => b.game_id === g.game_id).map((b) => el('div', { class: 'person' },
    emblem(b, 44),
    el('div', { class: 'what' }, el('b', {}, b.name), el('small', { class: 'soft' }, `${b.description ? `${b.description} · ` : ''}${count(b.earned_by)} earned`), el('code', {}, b.badge_id)),
    el('div', { class: 'buttons' },
      el('button', { class: 'chip', disabled: mine.busy, onclick: () => { Object.assign(f, { name: b.name, description: b.description, icon: b.icon, color: b.color, editing: b.badge_id }); render(); } }, 'Edit'),
      el('button', { class: 'chip', disabled: mine.busy, onclick: (e) => {
        const btn = e.currentTarget;
        if (btn.dataset.sure) changeBadge(g.game_id, 'remove', b, `Removed ${b.name}.`);
        else { btn.dataset.sure = '1'; btn.replaceChildren(icon(ICON.trash), ' Remove?'); }
      } }, icon(ICON.trash), 'Remove'))));
  return [
    rows.length ? el('div', { class: 'versions' }, rows) : el('p', { class: 'soft' }, 'No badges yet.'),
    el('div', { class: 'form manage' },
      el('div', { class: 'badge-preview' }, emblem({ badge_id: 'new', icon: f.icon, color: f.color }, 64)),
      el('label', { class: 'field' }, el('span', {}, 'Name'), name),
      el('label', { class: 'field' }, el('span', {}, 'Description'), about),
      el('label', { class: 'field' }, el('span', {}, 'Color'), color),
      el('div', { class: 'field' }, el('span', {}, 'Icon'), icons),
      el('div', { class: 'form-actions' },
        el('button', { class: 'btn go small', disabled: mine.busy, onclick: save }, icon(ICON.check), f.editing ? 'Save' : 'Make badge'),
        f.editing && el('button', { class: 'chip', onclick: () => { Object.assign(f, { name: '', description: '', editing: null }); render(); } }, 'Cancel'))),
  ];
}

const size = (n) => (n < 1024 ? `${n} B` : n < 1048576 ? `${Math.round(n / 1024)} KB` : `${(n / 1048576).toFixed(1)} MB`);

function myGameCard(g) {
  const bits = [g.private ? 'Private' : 'Public', `v${g.version}`];
  if (g.playing) bits.push(`${count(g.playing)} playing`);
  return el('a', { class: 'game', href: `/creator/${encodeURIComponent(g.game_id)}` },
    el('div', { class: 'pic' }, gameIcon(188, 150, g.game_id, g),
      g.private && el('span', { class: 'badge', style: `background:${rgb(pictureColour(g.game_id))}` }, icon(ICON.lock))),
    el('h3', {}, g.title),
    el('div', { class: `meta${g.playing ? ' live' : ' idle'}` }, bits.join(' · ')));
}

const minutes = (secs) => (secs < 60 ? `${secs}s` : secs < 3600 ? `${Math.round(secs / 60)} min` : `${(secs / 3600).toFixed(1)} h`);
const percent = (r) => (r === null || r === undefined ? '–' : `${Math.round(r * 100)}%`);
const dayName = (day) => new Date(day * 86400000).toLocaleDateString(undefined, { month: 'short', day: 'numeric', timeZone: 'UTC' });
const clock = (secs) => new Date(secs * 1000).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });

function chart(points, { bars = false, label }) {
  const w = 640, h = 170, l = 34, b = 22, t = 8;
  const most = Math.max(2, ...points.map((p) => p.v));
  const step = Math.pow(10, Math.floor(Math.log10(most)));
  const top = Math.ceil(most / step) * step;
  const x = (i) => l + ((w - l - 4) * (bars ? i + 0.5 : i)) / Math.max(1, bars ? points.length : points.length - 1);
  const y = (v) => t + (h - t - b) * (1 - v / top);
  const parts = [];
  for (const v of [0, top / 2, top]) {
    parts.push(svg('line', { x1: l, x2: w - 4, y1: y(v), y2: y(v), class: 'grid' }));
    parts.push(svg('text', { x: l - 6, y: y(v), class: 'axis', 'text-anchor': 'end', 'dominant-baseline': 'central' }, document.createTextNode(count(Math.round(v)))));
  }
  const every = Math.max(1, Math.ceil(points.length / 7));
  points.forEach((p, i) => {
    if (i % every === 0) parts.push(svg('text', { x: x(i), y: h - 6, class: 'axis', 'text-anchor': 'middle' }, document.createTextNode(p.short)));
  });
  const slot = (w - l - 4) / Math.max(1, points.length);
  if (bars) {
    const bw = Math.max(2, Math.min(28, slot - 2));
    points.forEach((p, i) => {
      const top = y(p.v);
      const ht = Math.max(0, h - b - top);
      parts.push(svg('path', { class: 'mark', d: ht ? `M${x(i) - bw / 2},${h - b}V${top + Math.min(4, ht)}q0,-${Math.min(4, ht)} ${Math.min(4, bw / 2)},-${Math.min(4, ht)}H${x(i) + bw / 2 - Math.min(4, bw / 2)}q${Math.min(4, bw / 2)},0 ${Math.min(4, bw / 2)},${Math.min(4, ht)}V${h - b}Z` : '' }));
    });
  } else {
    const line = points.map((p, i) => `${i ? 'L' : 'M'}${x(i)},${y(p.v)}`).join('');
    parts.push(svg('path', { class: 'area', d: `${line}L${x(points.length - 1)},${h - b}L${x(0)},${h - b}Z` }));
    parts.push(svg('path', { class: 'line', d: line }));
  }
  const hover = svg('g', { class: 'hover' });
  parts.push(hover);
  const tip = el('div', { class: 'tip' });
  const box = el('div', { class: 'chart', role: 'img', 'aria-label': label });
  const area = svg('svg', { viewBox: `0 0 ${w} ${h}` }, parts);
  area.addEventListener('pointermove', (e) => {
    const r = area.getBoundingClientRect();
    const px = ((e.clientX - r.left) / r.width) * w;
    const i = Math.max(0, Math.min(points.length - 1, Math.round(bars ? (px - l) / slot - 0.5 : ((px - l) / (w - l - 4)) * (points.length - 1))));
    const p = points[i];
    fill(hover, svg('line', { x1: x(i), x2: x(i), y1: t, y2: h - b, class: 'cross' }), !bars && svg('circle', { cx: x(i), cy: y(p.v), r: 4, class: 'dot' }));
    tip.replaceChildren(el('b', {}, count(p.v)), ' ', p.long);
    tip.style.left = `${(x(i) / w) * 100}%`;
    tip.hidden = false;
  });
  area.addEventListener('pointerleave', () => { hover.replaceChildren(); tip.hidden = true; });
  tip.hidden = true;
  return add(box, [area, tip]);
}

const credits = (n) => `${count(n)} Credits`;
const money = (cents) => `$${(cents / 100).toFixed(2)}`;

const passList = { key: null, who: null, list: null, busy: false };

function loadPasses(key, again) {
  if (!again && passList.key === key && passList.who === (session?.playerId || null)) return;
  passList.key = key;
  passList.who = session?.playerId || null;
  const query = session ? { player: session.playerId } : undefined;
  api(`/games/${encodeURIComponent(key)}/passes`, { query })
    .then((list) => { if (passList.key === key) passList.list = list; })
    .catch(() => { passList.list = []; })
    .finally(() => render());
}

async function buyPass(p) {
  passList.busy = true;
  render();
  const time = walletTime();
  const signature = signText(secretKey(), `libroxel buy pass\n${session.playerId}\n${p.pass_id}\n${p.price}\n${time}`);
  try {
    await api('/passes/buy', { method: 'POST', body: { player_id: session.playerId, pass_id: p.pass_id, price: p.price, time, signature } });
    flash = { text: `You have ${p.name}.` };
    loadPasses(p.game_id, true);
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  passList.busy = false;
  render();
}

function passesPanel(entry) {
  loadPasses(entry.key);
  const list = passList.key === entry.key ? passList.list : null;
  if (!list?.length) return null;
  return el('div', { class: 'about' }, el('div', { class: 'caps' }, 'Passes'),
    el('div', { class: 'versions' }, list.map((p) => el('div', { class: 'person' },
      el('div', { class: 'what' }, el('b', {}, p.name), p.about && el('small', { class: 'soft' }, p.about)),
      el('div', { class: 'buttons' }, p.owned
        ? el('span', { class: 'soft' }, 'Owned')
        : el('button', { class: 'chip', disabled: !session || passList.busy, title: session ? undefined : 'Sign in to buy passes', onclick: (e) => {
          const b = e.currentTarget;
          if (b.dataset.sure) buyPass(p);
          else { b.dataset.sure = '1'; b.replaceChildren(`Buy for ${credits(p.price)}?`); }
        } }, credits(p.price)))))));
}

// --- Private servers: a server of a game for its owner and their friends ---

let privates = { who: null, list: null, busy: false, name: '' };

function loadPrivateServers() {
  if (!session) return null;
  if (privates.who === session.playerId) return privates.list;
  privates = { who: session.playerId, list: null, busy: false, name: '' };
  const time = Date.now();
  const signature = signText(secretKey(), `playlibre private servers\n${session.playerId}\n${time}`);
  api(`/private-servers/${session.playerId}`, { query: { time, signature } })
    .then((list) => { privates.list = list; render(); })
    .catch(() => { privates.list = []; });
  return null;
}

async function changePrivateServer(action, gameId, name, code, said) {
  if (privates.busy) return;
  privates.busy = true;
  render();
  const time = Math.max(Date.now(), lastSigned.privateServer + 1);
  lastSigned.privateServer = time;
  const body = { player_id: session.playerId, action, game_id: gameId, name, code, time };
  body.signature = signText(secretKey(), `playlibre private server\n${session.playerId}\n${action}\n${gameId}\n${name}\n${code}\n${time}`);
  try {
    privates.list = await api('/private-servers', { method: 'POST', body });
    privates.name = '';
    flash = { text: said };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  privates.busy = false;
  render();
}

function privateServersPanel(entry) {
  if (!session || entry.game?.private) return null;
  const list = (loadPrivateServers() || []).filter((p) => p.game_id === entry.key);
  const name = el('input', { type: 'text', maxlength: 50, value: privates.name, placeholder: `Such as ${session.name || 'My'}'s server`, oninput: (e) => { privates.name = e.target.value; } });
  return el('div', { class: 'about' }, el('div', { class: 'caps' }, 'Private servers'),
    el('p', { class: 'note' }, 'A server of your own: only you and your friends can play on it.'),
    list.length ? el('div', { class: 'versions' }, list.map((p) => el('div', { class: 'person' },
      el('div', { class: 'what' }, el('b', {}, p.name), el('small', { class: 'soft' }, `${p.owner_id === session.playerId ? 'Yours' : `${p.owner}'s`}${p.players ? ` · ${p.players} playing` : ''}`)),
      el('div', { class: 'buttons' },
        el('button', { class: 'chip', onclick: () => launch(appLink('private', `${entry.key}:${p.code}`), `Joining ${p.name}.`) }, icon(ICON.play), 'Play'),
        p.owner_id === session.playerId && el('button', { class: 'chip', disabled: privates.busy, onclick: (e) => {
          const b = e.currentTarget;
          if (b.dataset.sure) changePrivateServer('remove', '', '', p.code, `Removed ${p.name}.`);
          else { b.dataset.sure = '1'; b.replaceChildren(icon(ICON.trash), ' Remove?'); }
        } }, icon(ICON.trash), 'Remove'))))) : null,
    el('div', { class: 'form-actions' }, name,
      el('button', { class: 'btn small', disabled: privates.busy, onclick: () => name.value.trim() && changePrivateServer('create', entry.key, name.value.trim(), '', `Made ${name.value.trim()}. Your friends see it here too.`) }, icon(ICON.check), 'Make one')));
}

async function changePass(gameId, c, said) {
  if (mine.busy) return;
  mine.busy = true;
  render();
  const body = { player_id: session.playerId, action: c.action, pass_id: c.pass_id || 0, name: c.name || '', about: c.about || '', price: c.price || 0, time: walletTime() };
  body.signature = signText(secretKey(), `libroxel pass\n${body.player_id}\n${gameId}\n${body.action}\n${body.pass_id}\n${body.name}\n${body.about}\n${body.price}\n${body.time}`);
  try {
    passList.list = await api(`/games/${encodeURIComponent(gameId)}/passes`, { method: 'POST', body });
    passList.key = gameId;
    passList.who = session.playerId;
    flash = { text: said };
    earned.for = null;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  mine.busy = false;
  render();
}

function passForm(gameId, p) {
  const name = el('input', { type: 'text', value: p?.name || '', maxlength: 50 });
  const about = el('input', { type: 'text', value: p?.about || '', maxlength: 300 });
  const price = el('input', { type: 'number', min: 1, max: 100000, value: p?.price || '' });
  const read = () => ({ pass_id: p?.pass_id, name: name.value.trim(), about: about.value.trim(), price: Math.floor(Number(price.value) || 0) });
  return el('div', { class: 'form manage' },
    el('label', { class: 'field' }, el('span', {}, 'Name'), name),
    el('label', { class: 'field' }, el('span', {}, 'Description'), about),
    el('label', { class: 'field' }, el('span', {}, 'Price in Credits'), price),
    el('div', { class: 'form-actions' },
      el('button', { class: 'btn go small', disabled: mine.busy, onclick: () => changePass(gameId, { action: p ? 'edit' : 'add', ...read() }, p ? 'Saved.' : `Added ${name.value.trim()}.`) }, icon(ICON.check), p ? 'Save' : 'Add pass'),
      p && el('button', { class: 'chip', disabled: mine.busy, onclick: () => changePass(gameId, { action: 'remove', pass_id: p.pass_id }, `Removed ${p.name}.`) }, icon(ICON.trash), 'Remove')));
}

function passesTab(g) {
  loadPasses(g.game_id);
  const list = passList.key === g.game_id ? passList.list : null;
  if (!list) return el('p', { class: 'soft' }, 'Loading…');
  return el('div', {},
    list.map((p) => el('div', {}, el('h2', { class: 'section' }, `${p.name} · ${credits(p.price)} · ${count(p.sold)} sold`), passForm(g.game_id, p))),
    el('h2', { class: 'section' }, 'New pass'), passForm(g.game_id, null));
}

const productList = { key: null, list: null };

function loadProducts(key, again) {
  if (!again && productList.key === key) return;
  productList.key = key;
  productList.list = null;
  api(`/games/${encodeURIComponent(key)}/products`)
    .then((list) => { if (productList.key === key) productList.list = list; })
    .catch(() => { productList.list = []; })
    .finally(() => render());
}

async function changeProduct(gameId, c, said) {
  if (mine.busy) return;
  mine.busy = true;
  render();
  const body = { player_id: session.playerId, action: c.action, product_id: c.product_id || 0, name: c.name || '', about: c.about || '', price: c.price || 0, time: walletTime() };
  body.signature = signText(secretKey(), `playlibre product\n${body.player_id}\n${gameId}\n${body.action}\n${body.product_id}\n${body.name}\n${body.about}\n${body.price}\n${body.time}`);
  try {
    productList.list = await api(`/games/${encodeURIComponent(gameId)}/products`, { method: 'POST', body });
    productList.key = gameId;
    flash = { text: said };
    earned.for = null;
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  mine.busy = false;
  render();
}

function productForm(gameId, p) {
  const name = el('input', { type: 'text', value: p?.name || '', maxlength: 50 });
  const about = el('input', { type: 'text', value: p?.about || '', maxlength: 300 });
  const price = el('input', { type: 'number', min: 1, max: 100000, value: p?.price || '' });
  const read = () => ({ product_id: p?.product_id, name: name.value.trim(), about: about.value.trim(), price: Math.floor(Number(price.value) || 0) });
  return el('div', { class: 'form manage' },
    el('label', { class: 'field' }, el('span', {}, 'Name'), name),
    el('label', { class: 'field' }, el('span', {}, 'Description'), about),
    el('label', { class: 'field' }, el('span', {}, 'Price in Credits'), price),
    el('div', { class: 'form-actions' },
      el('button', { class: 'btn go small', disabled: mine.busy, onclick: () => changeProduct(gameId, { action: p ? 'edit' : 'add', ...read() }, p ? 'Saved.' : `Added ${name.value.trim()}.`) }, icon(ICON.check), p ? 'Save' : 'Add product'),
      p && el('button', { class: 'chip', disabled: mine.busy, onclick: () => changeProduct(gameId, { action: 'remove', product_id: p.product_id }, `Removed ${p.name}.`) }, icon(ICON.trash), 'Remove')));
}

function productsTab(g) {
  loadProducts(g.game_id);
  const list = productList.key === g.game_id ? productList.list : null;
  if (!list) return el('p', { class: 'soft' }, 'Loading…');
  return el('div', {},
    list.map((p) => el('div', {}, el('h2', { class: 'section' }, `${p.name} · #${p.product_id} · ${credits(p.price)} · ${count(p.sold)} sold`), productForm(g.game_id, p))),
    el('h2', { class: 'section' }, 'New product'), productForm(g.game_id, null));
}

const earned = { result: null, loading: false, error: null, for: null, busy: false };

function loadEarnings(again) {
  if (again) earned.for = null;
  if (earned.loading || earned.for === session.playerId) return;
  earned.loading = true;
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel earnings\n${session.playerId}\n${time}`);
  api(`/earnings/${session.playerId}`, { query: { time, signature } })
    .then((r) => { earned.result = r; earned.error = null; })
    .catch((e) => { earned.error = e.message; })
    .finally(() => { earned.loading = false; earned.for = session.playerId; if (route.name === 'creator') render(); });
}

async function cashOut(amount, details) {
  earned.busy = true;
  render();
  const body = { player_id: session.playerId, credits: amount, details, time: walletTime() };
  body.signature = signText(secretKey(), `libroxel cash out\n${body.player_id}\n${body.credits}\n${body.details}\n${body.time}`);
  try {
    earned.result = await api('/earnings/cash-out', { method: 'POST', body });
    flash = { text: 'Cash out asked for.' };
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  earned.busy = false;
  render();
}

function cashOutPanel(e) {
  const out = [el('h2', { class: 'section' }, 'Cash out')];
  if (!e.payouts_on) return [...out, el('p', { class: 'soft' }, 'Not available yet.')];
  out.push(el('p', { class: 'soft' }, `${money(e.cents_per_thousand)} per 1,000 Credits through ${e.provider}. At least ${credits(e.minimum)}. You can cash out ${credits(e.payable)}.`));
  if (e.needs.length) out.push(status(`First ${e.needs.join(', ')}.`));
  else if (!e.payouts.some((p) => p.state === 'waiting')) {
    const amount = el('input', { type: 'number', min: e.minimum, max: e.payable, value: e.payable });
    const details = el('input', { type: 'text', maxlength: 200, placeholder: `Your ${e.provider} email or account` });
    out.push(el('div', { class: 'form manage' },
      el('label', { class: 'field' }, el('span', {}, 'Credits'), amount),
      el('label', { class: 'field' }, el('span', {}, 'Pay to'), details),
      el('div', { class: 'form-actions' },
        el('button', { class: 'btn go small', disabled: earned.busy || e.payable < e.minimum, onclick: () => cashOut(Math.floor(Number(amount.value) || 0), details.value.trim()) }, icon(ICON.check), 'Cash out'))));
  }
  if (e.payouts.length) {
    out.push(el('div', { class: 'versions' }, e.payouts.map((p) => el('div', { class: 'person' },
      el('div', { class: 'what' }, el('b', {}, `${credits(p.credits)} · ${money(p.cents)}`), el('small', { class: 'soft' }, [ago(p.asked_secs_ago), p.note].filter(Boolean).join(' · '))),
      el('div', { class: 'buttons' }, el('span', { class: 'soft' }, { waiting: 'Waiting', paid: 'Paid', refused: 'Refused' }[p.state] || p.state))))));
  }
  return out;
}

function earningsPage() {
  loadEarnings();
  const out = [el('a', { class: 'back', href: '/creator' }, icon(ICON.left), 'Create'), el('h1', { class: 'title' }, 'Earnings')];
  const e = earned.result;
  if (earned.error) out.push(status(earned.error, 'error'));
  if (!e) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  const tiles = [[credits(e.balance), 'Balance'], [credits(e.pending), 'On hold'], [credits(e.last_30), 'Last 30 days'], [credits(e.total), 'All time']];
  out.push(el('div', { class: 'tiles' }, tiles.map(([v, l]) => el('div', {}, el('b', {}, v), el('span', { class: 'caps' }, l)))));
  out.push(el('h2', { class: 'section' }, 'Each day'),
    chart(e.days.map((d) => ({ v: d.sales + d.engagement, short: dayName(d.day), long: `${dayName(d.day)}: ${credits(d.sales)} sales, ${credits(d.engagement)} playtime` })), { bars: true, label: 'Credits earned each day' }));
  if (e.games.length) {
    out.push(el('h2', { class: 'section' }, 'Games'), el('div', { class: 'table-wrap' }, el('table', { class: 'numbers' },
      el('thead', {}, el('tr', {}, ['Game', 'Passes', 'Playtime', 'Total'].map((h) => el('th', {}, h)))),
      el('tbody', {},
        e.games.map((g) => el('tr', {},
          el('td', {}, el('a', { href: `/creator/${encodeURIComponent(g.game_id)}/passes` }, g.title)),
          [g.passes, g.engagement, g.passes + g.engagement].map((v) => el('td', {}, count(v))))),
        e.items > 0 && el('tr', {}, el('td', {}, 'Items'), el('td', {}, '–'), el('td', {}, '–'), el('td', {}, count(e.items)))))));
  }
  if (e.sales.length) {
    out.push(el('h2', { class: 'section' }, 'Sales'), el('div', { class: 'versions' }, e.sales.map((s) => el('div', { class: 'person' },
      el('div', { class: 'what' }, el('b', {}, s.name), el('small', { class: 'soft' }, `${credits(s.price)} · ${ago(s.secs_ago)}`)),
      el('div', { class: 'buttons' }, el('span', { class: 'soft' }, `${s.settled ? '+' : 'On hold: '}${credits(s.earned)}`))))));
  }
  out.push(...cashOutPanel(e));
  return out;
}

const PERMISSION_WORDS = { add: 'Add new things', change: "Change, move or delete what's there", scripts: 'Write or change scripts' };
const market = { key: null, list: null, error: null, kind: '', sort: '', q: '', item: null };

function loadMarket() {
  const key = `${market.kind}|${market.sort}|${market.q}`;
  if (market.key === key) return;
  market.key = key;
  const query = { q: market.q };
  if (market.kind) query.kind = market.kind;
  if (market.sort) query.sort = market.sort;
  api('/market', { query })
    .then((list) => { market.list = list; market.error = null; })
    .catch((e) => { market.error = e.message; })
    .finally(() => route.name === 'market' && render());
}

function loadMarketItem(id) {
  if (market.item?.asset_id === id || market.item?.loading === id) return;
  market.item = { loading: id };
  api(`/market/${encodeURIComponent(id)}`)
    .then((item) => { market.item = item; })
    .catch((e) => { market.item = { asset_id: id, error: e.message }; })
    .finally(() => route.name === 'market' && render());
}

function permissionsLine(item) {
  if (item.kind !== 'plugin') return null;
  return el('div', { class: 'soft' }, item.permissions.length ? item.permissions.map((p) => PERMISSION_WORDS[p] || p).join(', ') : 'Can only look');
}

async function takeDown(item) {
  const time = nowSecs();
  const signature = signText(secretKey(), `libroxel unmarket\n${item.asset_id}\n${time}`);
  try {
    await api(`/market/${encodeURIComponent(item.asset_id)}`, { method: 'DELETE', body: { time, signature } });
    flash = { text: 'Taken down.' };
    market.key = null;
    market.item = null;
    history.pushState(null, '', '/market');
  } catch (e) {
    flash = { text: e.message, kind: 'error' };
  }
  render();
}

function marketItemPage(id) {
  loadMarketItem(id);
  const item = market.item;
  const out = [el('a', { class: 'back', href: '/market' }, icon(ICON.left), 'Marketplace')];
  if (!item || item.loading) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  if (item.error) return [...out, el('div', { class: 'empty' }, item.error)];
  const mine = session && session.playerId === item.creator_id;
  out.push(el('h1', { class: 'title' }, item.name),
    el('div', { class: 'soft' }, item.kind === 'plugin' ? 'Plugin' : 'Model', ' by ', el('a', { href: `/user/${item.creator_id}` }, item.creator),
      ` · version ${item.version} · ${count(item.installs)} uses · ${ago(item.updated_secs_ago)}`),
    item.about && el('p', {}, item.about),
    permissionsLine(item),
    el('p', { class: 'soft' }, item.price ? 'Paid' : 'Free', ' · Open PlayLibre, then Model › Marketplace in the editor to use it.'),
    el('div', { class: 'buttons' },
      session && !mine && el('button', { class: 'chip', onclick: () => startReport('market', item.asset_id, item.name) }, icon(ICON.flag), 'Report'),
      mine && el('button', { class: 'chip', onclick: () => takeDown(item) }, icon(ICON.trash), 'Take down')),
    reportPanel('market', item.asset_id));
  return out;
}

const shop = { key: null, list: null, error: null, q: '', kind: '', price: '', sort: '', item: null, busy: false };

function loadShop() {
  const key = `${shop.kind}|${shop.price}|${shop.sort}|${shop.q}`;
  if (shop.key === key) return;
  shop.key = key;
  const query = { q: shop.q };
  for (const k of ['kind', 'price', 'sort']) if (shop[k]) query[k] = shop[k];
  api('/catalog', { query })
    .then((list) => { if (shop.key === key) { shop.list = list; shop.error = null; } })
    .catch((e) => { shop.error = e.message; })
    .finally(() => route.name === 'market' && render());
}

function loadShopItem(id) {
  if (shop.item?.item_id === id || shop.item?.loading === id) return;
  shop.item = { loading: id };
  api(`/catalog/${encodeURIComponent(id)}`)
    .then((item) => { shop.item = item; })
    .catch((e) => { shop.item = { item_id: id, error: e.message }; })
    .finally(() => route.name === 'item' && render());
}

const priceTag = (item) => el('div', { class: 'price' }, item.price ? [icon(BADGE_GLYPHS.coin), count(item.price)] : 'Free');
const creatorLink = (item) => (item.creator_id === 'playlibre' ? item.creator : el('a', { href: `/user/${item.creator_id}` }, item.creator));

function itemCard(item) {
  return el('div', { class: 'shop-item' },
    el('a', { href: href('item', item.item_id) }, itemPicture(item), el('b', {}, item.name)),
    el('small', { class: 'soft' }, `${KIND_NAMES[item.kind] || item.kind} by ${item.creator}`),
    priceTag(item),
    getButton(item));
}

function limitedFields(item) {
  const stock = el('input', { type: 'number', min: 1, max: 100000, placeholder: 'Stock', style: 'width:80px' });
  const days = el('input', { type: 'number', min: 1, max: 90, placeholder: 'Days', style: 'width:70px' });
  return [stock, days, el('button', { class: 'chip', onclick: () => makeLimited(item, Math.floor(Number(stock.value) || 0), Math.floor(Number(days.value) || 0)) }, 'Make limited')];
}

function itemPage() {
  const id = route.id;
  loadShopItem(id);
  if (session) loadWallet();
  const item = shop.item;
  const out = [el('a', { class: 'back', href: '/market' }, icon(ICON.left), 'Marketplace')];
  if (!item || item.loading) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  if (item.error) return [...out, el('div', { class: 'empty' }, item.error)];
  const mine = session && session.playerId === item.creator_id;
  const box = el('input', { type: 'number', min: 0, max: 100000, value: item.price || 0, style: 'width:90px' });
  out.push(el('div', { class: 'avatar-page' },
    el('div', { class: 'figure' }, itemPicture(item, 240)),
    el('div', { style: 'flex:1;min-width:260px' },
      el('h1', { class: 'title' }, item.name),
      el('div', { class: 'soft' }, KIND_NAMES[item.kind] || item.kind, ' by ', creatorLink(item), ` · worn ${count(item.wears)}`),
      priceTag(item),
      item.limited && el('div', { class: 'soft' }, limitedText(item.limited)),
      session && walletBar(),
      el('div', { class: 'buttons' },
        getButton(item),
        session && !mine && el('button', { class: 'chip', onclick: () => startReport('item', item.item_id, item.name) }, icon(ICON.flag), 'Report')),
      mine && el('div', { class: 'form-actions' }, box,
        el('button', { class: 'chip', onclick: () => priceItem(item, Math.max(0, Math.min(100000, Math.floor(Number(box.value) || 0)))) }, 'Set price')),
      mine && !item.limited && item.price > 0 && el('div', { class: 'form-actions' }, limitedFields(item)),
      item.limited && session && salesPanel(item.item_id),
      reportPanel('item', item.item_id))));
  return out;
}

function itemList() {
  loadShop();
  if (session) loadWallet();
  const box = el('input', { type: 'search', placeholder: 'Search', value: shop.q });
  box.addEventListener('change', () => { shop.q = box.value.trim(); render(); });
  const chips = (list, key) => el('div', { class: 'hats' }, list.map(([value, label]) =>
    el('button', { class: `chip${shop[key] === value ? ' on' : ''}`, onclick: () => { shop[key] = value; render(); } }, label)));
  const out = [box,
    chips([['', 'Any price'], ['free', 'Free'], ['paid', 'Paid']], 'price'),
    chips([['', 'Popular'], ['new', 'New'], ['cheap', 'Lowest price'], ['dear', 'Highest price'], ['name', 'Name']], 'sort'),
    session && walletBar()];
  if (shop.error) out.push(status(shop.error, 'error'));
  else if (!shop.list) out.push(el('p', { class: 'soft' }, 'Loading…'));
  else if (!shop.list.length) out.push(el('div', { class: 'empty' }, 'Nothing here yet.'));
  else out.push(el('div', { class: 'shop' }, shop.list.map(itemCard)));
  if (session) out.push(limitedPanel());
  if (mayMakeItems()) out.push(clothesMaker());
  return out;
}

function assetList() {
  loadMarket();
  const box = el('input', { type: 'search', placeholder: 'Search', value: market.q });
  box.addEventListener('change', () => { market.q = box.value.trim(); render(); });
  const out = [box, el('div', { class: 'hats' }, [['', 'Most used'], ['new', 'New'], ['updated', 'Updated']].map(([value, label]) =>
    el('button', { class: `chip${market.sort === value ? ' on' : ''}`, onclick: () => { market.sort = value; render(); } }, label)))];
  if (market.error) return [...out, status(market.error, 'error')];
  if (!market.list) return [...out, el('p', { class: 'soft' }, 'Loading…')];
  if (!market.list.length) return [...out, el('div', { class: 'empty' }, 'Nothing here yet.')];
  out.push(el('div', { class: 'market' }, market.list.map((item) => el('a', { class: 'market-row', href: `/market/${encodeURIComponent(item.asset_id)}` },
    el('b', {}, item.name),
    el('div', { class: 'soft' }, `${item.kind === 'plugin' ? 'Plugin' : 'Model'} by ${item.creator} · ${count(item.installs)} uses`),
    permissionsLine(item)))));
  return out;
}

const SHOP_KINDS = [['', 'All'], ['hat', 'Hats'], ['shirt', 'Shirts'], ['pants', 'Trousers'], ['model', 'Models'], ['plugin', 'Plugins']];

function marketPage() {
  if (route.id) return marketItemPage(route.id);
  const assets = shop.kind === 'model' || shop.kind === 'plugin';
  const out = [el('h1', { class: 'title' }, 'Marketplace'),
    el('div', { class: 'hats' }, SHOP_KINDS.map(([value, label]) =>
      el('button', { class: `chip${shop.kind === value ? ' on' : ''}`, onclick: () => { shop.kind = value; if (value === 'model' || value === 'plugin') market.kind = value; render(); } }, label)))];
  return [...out, ...(assets ? assetList() : itemList())];
}

function creatorPage() {
  if (!session) return needSignIn('Sign in to see how your games are played.');
  loadCreator();
  const range = el('div', { class: 'hats' }, [1, 7, 30, 90].map((d) =>
    el('button', { class: `chip${creator.days === d ? ' on' : ''}`, onclick: () => { creator.days = d; creator.for = null; render(); } }, d === 1 ? 'Today' : `${d} days`)));
  const games = creator.result?.games || [];
  const out = [];
  if (route.id === 'earnings') return earningsPage();
  loadMine();
  if (route.id) {
    const g = games.find((x) => x.game_id === route.id);
    const info = mine.list?.find((x) => x.game_id === route.id);
    out.push(el('a', { class: 'back', href: '/creator' }, icon(ICON.left), 'Create'));
    if (!info && route.tab === 'data') {
      out.push(el('h1', { class: 'title' }, 'Saved data'), dataTab(route.id));
      return out;
    }
    if (!info) {
      out.push(mine.list ? el('div', { class: 'empty' }, "That isn't one of your games.") : el('p', { class: 'soft' }, 'Loading…'));
      return out;
    }
    const base = `/creator/${encodeURIComponent(info.game_id)}`;
    const tab = ['settings', 'passes', 'products', 'versions', 'badges', 'data'].includes(route.tab) ? route.tab : '';
    out.push(el('div', { class: 'manage-head' },
      el('div', { class: 'mini' }, gameIcon(64, 64, info.game_id, info)),
      el('div', {}, el('h1', { class: 'title' }, info.title),
        el('div', { class: 'soft' }, `${info.private ? 'Private' : 'Public'} · version ${info.version} · ${ago(info.updated_secs_ago)}`)),
      !info.private && el('a', { class: 'chip', href: gameHrefOf(info.game_id, info.number, info.title) }, icon(ICON.play), 'View')));
    out.push(el('div', { class: 'hats' }, [['', 'Numbers'], ['settings', 'Settings'], ['passes', 'Passes'], ['products', 'Products'], ['versions', 'Versions'], ['badges', 'Badges'], ['data', 'Data']].map(([t, label]) =>
      el('a', { class: `chip${tab === t ? ' on' : ''}`, href: t ? `${base}/${t}` : base }, label))));
    if (tab === 'settings') { out.push(settingsTab(info)); return out; }
    if (tab === 'passes') { out.push(passesTab(info)); return out; }
    if (tab === 'products') { out.push(productsTab(info)); return out; }
    if (tab === 'versions') { out.push(versionsTab(info)); return out; }
    if (tab === 'badges') { out.push(badgesTab(info)); return out; }
    if (tab === 'data') { out.push(dataTab(info.game_id)); return out; }
    out.push(range);
    if (!g) {
      out.push(creator.error ? status(creator.error, 'error') : el('p', { class: 'soft' }, 'Loading…'));
      return out;
    }
    const tiles = [
      [count(g.visits), 'Visits'], [count(g.players), 'Players'], [count(g.playing), 'Playing now'], [count(g.peak), 'Most at once'],
      [minutes(g.average_playtime_secs), 'Average playtime'], [percent(g.day1_retention), 'Day 1 back'], [percent(g.day7_retention), 'Day 7 back'],
      [count(g.likes), 'Likes'], [count(g.favorites), 'Favorites'],
    ];
    out.push(el('div', { class: 'tiles' }, tiles.map(([v, l]) => el('div', {}, el('b', {}, v), el('span', { class: 'caps' }, l)))));
    const every = g.concurrent_every_secs;
    const from = creator.result.from;
    out.push(el('h2', { class: 'section' }, 'Players online'),
      chart(g.concurrent.map((v, i) => ({ v, short: creator.days === 1 ? new Date((from + i * every) * 1000).toLocaleTimeString(undefined, { hour: 'numeric' }) : dayName(Math.floor((from + i * every) / 86400)), long: clock(from + i * every) })), { label: 'Players online over time' }));
    const daily = (key, label) => chart(g.days.map((d) => ({ v: d[key], short: dayName(d.day), long: dayName(d.day) })), { bars: true, label });
    if (creator.days > 1) {
      out.push(el('h2', { class: 'section' }, 'Visits'), daily('visits', 'Visits each day'));
      out.push(el('h2', { class: 'section' }, 'Players'), daily('players', 'Players each day'));
      out.push(el('h2', { class: 'section' }, 'New players'), daily('new_players', 'New players each day'));
      out.push(el('h2', { class: 'section' }, 'Likes'), daily('likes', 'Likes each day'));
    }
    return out;
  }
  out.push(el('div', { class: 'servers-head' }, el('h1', { class: 'title' }, 'Create'),
    el('a', { class: 'chip', href: '/creator/earnings' }, icon(ICON.chart), 'Earnings')));
  if (mine.error) out.push(status(mine.error, 'error'));
  if (!mine.list) out.push(el('p', { class: 'soft' }, 'Loading…'));
  else if (!mine.list.length) out.push(el('div', { class: 'empty' }, 'No published games yet.'));
  else out.push(el('div', { class: 'games' }, mine.list.map(myGameCard)));
  if (!mine.list?.length) return out;
  out.push(el('h2', { class: 'section' }, 'Numbers'), range);
  if (creator.error) out.push(status(creator.error, 'error'));
  if (!creator.result) out.push(el('p', { class: 'soft' }, 'Loading…'));
  else if (games.length) {
    const head = ['Game', 'Visits', 'Players', 'Playtime', 'Day 1', 'Day 7', 'Likes', 'Favorites'];
    out.push(el('div', { class: 'table-wrap' }, el('table', { class: 'numbers' },
      el('thead', {}, el('tr', {}, head.map((h) => el('th', {}, h)))),
      el('tbody', {}, games.map((g) => el('tr', {},
        el('td', {}, el('a', { href: `/creator/${encodeURIComponent(g.game_id)}` }, g.title)),
        [count(g.visits), count(g.players), minutes(g.average_playtime_secs), percent(g.day1_retention), percent(g.day7_retention), count(g.likes), count(g.favorites)].map((v) => el('td', {}, v))))))));
  }
  return out;
}

let familyInfo = null;
let familyMessage = null;
let parentPin = null;

function loadFamily() {
  if (!session || (familyInfo && familyInfo.for === session.playerId)) return;
  familyInfo = { for: session.playerId, loading: true };
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel family read\n${session.playerId}\n${time}`);
  api(`/accounts/${session.playerId}/family`, { query: { time, signature } })
    .then((f) => { familyInfo = { for: session.playerId, ...f }; })
    .catch((e) => { familyInfo = { for: session.playerId, error: e.message }; })
    .finally(() => render());
}

function ageOn(born) {
  const [y, m, d] = born.split('-').map(Number);
  const now = new Date();
  let age = now.getFullYear() - y;
  if (now.getMonth() + 1 < m || (now.getMonth() + 1 === m && now.getDate() < d)) age -= 1;
  return age;
}

async function setBirthday(secret, playerId, born, parentEmail) {
  const parent = (parentEmail || '').trim().toLowerCase();
  const offset = -new Date().getTimezoneOffset();
  const time = Date.now();
  const signature = signText(secret, `libroxel birthday\n${playerId}\n${born}\n${parent}\n${offset}\n${time}`);
  return api('/accounts/birthday', { method: 'POST', body: { player_id: playerId, born, parent_email: parent || null, utc_offset_mins: offset, time, signature } });
}

async function familyCall(work, done, after) {
  try {
    const f = await work();
    if (f) familyInfo = { for: session.playerId, ...f };
    else familyInfo = null;
    familyMessage = done ? { text: done } : null;
    after?.();
  } catch (e) {
    familyMessage = { text: e.message, kind: 'error' };
  }
  render();
}

function birthdayFields() {
  const born = el('input', { type: 'date', max: new Date().toISOString().slice(0, 10) });
  const parent = el('input', { type: 'email', maxlength: 254, placeholder: 'parent@example.com (optional)' });
  const parentField = el('label', { class: 'field', style: 'display:none' }, el('span', {}, "Parent's email"), parent);
  born.addEventListener('input', () => { parentField.style.display = born.value && ageOn(born.value) < 13 ? '' : 'none'; });
  const check = () => {
    if (!born.value) return 'Pick your birthday.';
    if (ageOn(born.value) < 13 && parent.value.trim() && !validEmail(parent.value)) return "That parent email doesn't look right.";
    return null;
  };
  const value = () => [born.value, ageOn(born.value) < 13 ? parent.value.trim() : null];
  return { nodes: [el('label', { class: 'field' }, el('span', {}, 'Birthday'), born), parentField], check, value };
}

function birthdayPage() {
  const fields = birthdayFields();
  const message = el('div', {});
  const form = el('form', { class: 'narrow' }, ...fields.nodes, el('div', { class: 'form-actions' }, el('button', { class: 'btn go', type: 'submit' }, 'Save')), message);
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const problem = fields.check();
    if (problem) return message.replaceChildren(status(problem, 'error'));
    const [born, parent] = fields.value();
    await familyCall(() => setBirthday(secretKey(), session.playerId, born, parent));
  });
  return [el('h1', { class: 'title' }, "When's your birthday?"), form, familyMessage && status(familyMessage.text, familyMessage.kind)];
}

function parentPanel() {
  loadFamily();
  const f = familyInfo;
  if (!f || f.loading || f.error || !f.has_birthday) return null;
  const panel = el('div', { class: 'panel', style: 'margin-top:26px' }, el('h3', {}, icon(ICON.lock), ' Parent settings'));
  const pinBox = (placeholder) => el('input', { type: 'password', inputmode: 'numeric', maxlength: 4, placeholder, style: 'max-width:120px' });
  const id = session.playerId;
  if (f.under_13 && !f.consent) {
    const code = el('input', { type: 'text', inputmode: 'numeric', maxlength: 6, placeholder: '6-digit code', style: 'max-width:140px' });
    const pin = pinBox("Parent's new PIN");
    const email = el('input', { type: 'email', maxlength: 254, placeholder: f.parent_email || 'parent@example.com' });
    add(panel, [
      el('p', { style: 'color:var(--amber)' }, 'Friend requests are off until a parent approves.'),
      el('div', { class: 'search' }, code, pin, el('button', { class: 'btn go small', onclick: () => familyCall(() => {
        const time = Date.now();
        const signature = signText(secretKey(), `libroxel consent\n${id}\n${code.value.trim()}\n${pin.value}\n${time}`);
        return api('/accounts/family/consent', { method: 'POST', body: { player_id: id, code: code.value.trim(), pin: pin.value, time, signature } });
      }, 'Approved.') }, 'Approve')),
      el('div', { class: 'search' }, email, el('button', { class: 'chip', onclick: () => familyCall(async () => {
        const to = (email.value.trim() || f.parent_email || '').toLowerCase();
        const time = Date.now();
        const signature = signText(secretKey(), `libroxel parent email\n${id}\n${to}\n${time}`);
        await api('/accounts/family/parent', { method: 'POST', body: { player_id: id, parent_email: to, time, signature } });
        return null;
      }, 'Sent.') }, 'Send code')),
    ]);
  } else if (parentPin === null) {
    const pin = pinBox(f.has_pin ? 'PIN' : 'New PIN');
    add(panel, [el('div', { class: 'search' }, pin, el('button', { class: 'btn go small', onclick: () => {
      if (!/^\d{4}$/.test(pin.value)) return;
      parentPin = f.has_pin ? { pin: pin.value, newPin: null } : { pin: '', newPin: pin.value };
      render();
    } }, f.has_pin ? 'Open' : 'Set PIN'))]);
  } else {
    const s = { ...f.settings };
    const choice = (key, options) => el('div', { class: 'hats' }, options.map(([value, label, off]) => el('button', { type: 'button', class: `chip${s[key] === value ? ' on' : ''}`, disabled: off, onclick: (e) => { s[key] = value; e.target.parentNode.querySelectorAll('.chip').forEach((c) => c.classList.toggle('on', c === e.target)); } }, label)));
    const friendsBox = el('input', { type: 'checkbox', checked: s.friend_requests });
    const limit = (value) => el('input', { type: 'text', inputmode: 'numeric', value: value ?? '', placeholder: 'No limit', style: 'max-width:120px' });
    const playtime = limit(s.playtime_mins);
    const spend = limit(s.spend_limit);
    const newPin = pinBox('New PIN');
    add(panel, [
      el('div', { class: 'caps' }, 'Chat'), choice('chat', [['off', 'Off'], ['strict', 'Strict'], ['standard', 'Standard', f.under_13]]),
      el('div', { class: 'caps' }, 'Messages'), choice('messages', [['everyone', 'Everyone'], ['friends', 'Friends'], ['off', 'Off']]),
      el('label', { class: 'check' }, friendsBox, 'Friend requests'),
      el('label', { class: 'field' }, el('span', {}, `Playtime a day, in minutes (played today: ${f.played_mins})`), playtime),
      el('label', { class: 'field' }, el('span', {}, 'Spending a month'), spend),
      el('label', { class: 'field' }, el('span', {}, 'New PIN'), newPin),
      el('div', { class: 'form-actions' },
        el('button', { class: 'btn go', onclick: () => {
          const num = (box) => (box.value === '' ? null : Math.max(0, Math.floor(Number(box.value))));
          const settings = { chat: s.chat, messages: s.messages, friend_requests: friendsBox.checked, spend_limit: num(spend), playtime_mins: num(playtime) };
          const fresh = parentPin.newPin || (/^\d{4}$/.test(newPin.value) ? newPin.value : null);
          const { pin } = parentPin;
          familyCall(() => {
            const time = Date.now();
            const signature = signText(secretKey(), `libroxel family\n${id}\n${pin}\n${fresh || ''}\n${JSON.stringify(settings)}\n${time}`);
            return api('/accounts/family', { method: 'POST', body: { player_id: id, pin, new_pin: fresh, settings, time, signature } });
          }, 'Saved.', () => { if (fresh) parentPin = { pin: fresh, newPin: null }; });
        } }, 'Save'),
        el('button', { class: 'chip', onclick: () => { parentPin = null; render(); } }, 'Close')),
    ]);
  }
  if (familyMessage) {
    panel.append(status(familyMessage.text, familyMessage.kind || ''));
    familyMessage = null;
  }
  return panel;
}

// --- Sign in, make an account -------------------------------------------------

let pending = null;
// What the other tab should start with, when one sends you to the other.
let carry = null;

/** The two tabs at the top of signing in and making an account. */
function accountTabs(current) {
  const tab = (name, href, label) => el('a', { class: `chip${current === name ? ' on' : ''}`, href }, label);
  return el('div', { class: 'hats', style: 'margin:4px 0 18px' }, tab('sign-in', '/sign-in', 'Sign in'), tab('create', '/create', 'Make an account'));
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
    el('div', { class: 'form-actions' }, go, !forced && el('a', { href: '/profile' }, 'Cancel')),
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
      goTo(forced ? '/' : '/profile');
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
    forced && el('p', {}, el('button', { class: 'link', type: 'button', onclick: () => { saveSession(null); data.friends = null; data.me = null; goTo('/sign-in'); } }, 'Sign out instead')),
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
      el('a', { href: '/' }, 'Go home')),
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
  const code = el('input', { type: 'text', inputmode: 'numeric', autocomplete: 'one-time-code', maxlength: 12, placeholder: '123456' });
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
    const say = (...text) => fill(message, el('div', { class: 'status wait' }, ...text));
    const fail = (text) => message.replaceChildren(status(text, 'error'));
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
      goTo(weak ? '/password' : '/');
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
        return fail(`${existing.name} has no password yet, so it can only be used on the computer it was made on. Open PlayLibre there and set a password on the Profile tab, then sign in here.`);
      }
      carry = { name: n, password: password.value };
      goTo('/create');
    }
  });
  return [
    el('h1', { class: 'title' }, 'Sign in'),
    accountTabs('sign-in'),
    el('p', { class: 'note' }, 'Use the name and password from the app. New here? Type the name you want and a password, and you can make the account next. Your password never leaves this computer: it is turned into a key here, the same way the app does it.'),
    form,
    resetForm(() => name.value.trim()),
  ];
}

const reset = { open: false, sent: false, needsCode: false };

function resetForm(nameNow) {
  if (!reset.open) return el('p', {}, el('button', { class: 'link', type: 'button', onclick: () => { reset.open = true; render(); } }, 'Forgot password?'));
  const name = el('input', { type: 'text', autocomplete: 'username', maxlength: 20, value: reset.name || nameNow() });
  const code = el('input', { type: 'text', inputmode: 'numeric', maxlength: 6, placeholder: '6-digit code' });
  const twoStepCode = el('input', { type: 'text', inputmode: 'numeric', autocomplete: 'one-time-code', maxlength: 12, placeholder: '123456' });
  const twoStepField = el('label', { class: 'field', style: reset.needsCode ? '' : 'display:none' }, el('span', {}, 'Code from your authenticator app, or a recovery code'), twoStepCode);
  const password = el('input', { type: 'password', autocomplete: 'new-password' });
  const again = el('input', { type: 'password', autocomplete: 'new-password' });
  const message = el('div');
  const say = (text, kind = 'wait') => message.replaceChildren(status(text, kind));
  const send = el('button', { class: 'chip', type: 'button' }, reset.sent ? 'Send again' : 'Email me a code');
  send.onclick = async () => {
    reset.name = name.value.trim();
    send.disabled = true;
    try {
      await api('/accounts/reset', { method: 'POST', body: { name: reset.name } });
      reset.sent = true;
      render();
    } catch (e) {
      say(e.message, 'error');
      send.disabled = false;
    }
  };
  const go = el('button', { class: 'btn go', type: 'submit' }, icon(ICON.lock), 'Reset password');
  const form = el('form', { class: 'narrow' },
    el('h3', {}, 'Reset your password'),
    el('label', { class: 'field' }, el('span', {}, 'Name'), name),
    el('div', { class: 'form-actions' }, send, el('button', { class: 'link', type: 'button', onclick: () => { reset.open = false; reset.sent = false; render(); } }, 'Cancel')),
    reset.sent && el('label', { class: 'field' }, el('span', {}, 'Code from the email'), code),
    reset.sent && twoStepField,
    reset.sent && el('label', { class: 'field' }, el('span', {}, 'New password'), password),
    reset.sent && el('label', { class: 'field' }, el('span', {}, 'New password again'), again),
    reset.sent && el('div', { class: 'form-actions' }, go),
    message);
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    go.disabled = true;
    try {
      await loadCommonPasswords();
      const n = name.value.trim();
      const problem = passwordProblem(password.value, n) || (password.value !== again.value ? "The passwords don't match." : null);
      if (problem) throw new Error(problem);
      say('Locking your account with your new password…');
      const done = await resetPassword(n, code.value, twoStepCode.value.trim(), password.value);
      const who = (await accountNamed(n).catch(() => null))?.name || n;
      saveSession({ ...done, name: who }, true);
      Object.assign(reset, { open: false, sent: false, needsCode: false, name: null });
      securityInfo = null;
      flash = { text: `Password reset. Welcome back, ${who}!` };
      await refresh();
      goTo('/');
    } catch (err) {
      go.disabled = false;
      if (err.status === 401) {
        reset.needsCode = true;
        twoStepField.style.display = '';
        message.replaceChildren();
        twoStepCode.focus();
        return;
      }
      say(err.message, 'error');
    }
  });
  return form;
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
  const birthday = birthdayFields();
  const remember = el('input', { type: 'checkbox', checked: true });
  const message = el('div', {}, from && status(`There's no account called ${from.name} yet. Type the password again to make it.`, 'wait'));
  const go = el('button', { class: 'btn go', type: 'submit' }, icon(ICON.userPlus), 'Make my account');
  const form = el('form', { class: 'narrow' },
    el('label', { class: 'field' }, el('span', {}, 'Name (3 to 20 letters, digits or _, with a letter)'), name),
    el('label', { class: 'field' }, el('span', {}, `Password (at least ${MIN_PASSWORD} characters, not a common one, without your name in it)`), password),
    el('label', { class: 'field' }, el('span', {}, 'Password again'), again),
    el('label', { class: 'field' }, el('span', {}, "Email (kept private: we mail it a code to prove it's yours)"), email),
    ...birthday.nodes,
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
        || (validEmail(email.value) ? null : 'Type your email: the Hub mails it a code to prove it is yours.')
        || birthday.check();
    if (problem) return say(problem, 'error');
    go.disabled = true;
    try {
      say('Checking the name is free…');
      const existing = await accountNamed(n);
      if (existing) throw new Error(`Someone already has the name ${existing.name} (names ignore capitals). If it's yours, sign in instead.`);
      const made = await createAccount(n, password.value, email.value.trim(), say, birthday.value());
      saveSession(made, remember.checked);
      securityInfo = null;
      flash = { text: `Welcome, ${n}! We mailed a code to ${email.value.trim()}: type it here to prove the email is yours.` };
      await refresh();
      goTo('/avatar');
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

const REPO = (document.querySelector('meta[name="playlibre-repo"]')?.content || '').trim();
let release = null; // The newest release with every installer on it, once GitHub has said.

const INSTALLERS = ['-windows-x86_64-setup.exe', '-macos-universal.dmg', '-linux-x86_64.AppImage', '-linux-x86_64.deb'];
const RELEASE_KEY = 'playlibre-release';
const assetFor = (r, end) => (r.assets || []).find((a) => a.name.startsWith('playlibre-') && a.name.endsWith(end));

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
    { id: 'windows', name: 'Windows', file: 'playlibre-windows-x86_64-setup.exe', ends: '-windows-x86_64-setup.exe', about: 'Windows 10 or 11. Open the setup wizard and press Next until it\'s done. PlayLibre then sits in the Start menu. If SmartScreen warns you, choose More info, then Run anyway.' },
    { id: 'macos', name: 'macOS', file: 'playlibre-macos-universal.dmg', ends: '-macos-universal.dmg', about: 'macOS 11 or newer. Open it and drag PlayLibre onto the Applications folder. The first time, open it, then click Open Anyway in System Settings, Privacy & Security.' },
    { id: 'linux', name: 'Linux', file: 'playlibre-linux-x86_64.AppImage', ends: '-linux-x86_64.AppImage', about: 'Any x86-64 Linux with Vulkan. Make the file executable (right-click, Properties, Allow running as a program) and open it. It adds itself to your app menu.',
      alt: { label: '.deb for Debian and Ubuntu', file: 'playlibre-linux-x86_64.deb', ends: '-linux-x86_64.deb' } },
  ].sort((a, b) => (b.id === mine) - (a.id === mine));
  // From the repository's newest release when there is one, else from this server.
  const link = (b) => {
    if (!REPO) return `downloads/${b.file}`;
    const asset = release?.assets?.find((a) => a.name.startsWith('playlibre-') && a.name.endsWith(b.ends));
    return asset ? asset.browser_download_url : null;
  };
  const out = [
    el('h1', { class: 'title' }, 'Download PlayLibre'),
    el('p', { class: 'note' }, 'PlayLibre is free. Get the app to play, make and host games.',
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
      el('li', {}, 'Install PlayLibre and open it once. That tells your computer that Play buttons on this site open PlayLibre, and typing playlibre in a terminal opens it too.'),
      el('li', {}, 'Come back here, pick a game, and press Play. Your browser may ask whether to open PlayLibre: say yes (and tick Always allow, if it offers).'),
      el('li', {}, 'Sign in to the app with the same name and password as here, so your friends see you.')),
  );
  return out;
}

// --- Donate ------------------------------------------------------------------

const PLANS = {
  once: { label: 'One-time', amounts: [5, 10, 25, 50, 100], per: '' },
  month: { label: 'Monthly', amounts: [3, 5, 10, 25, 50], per: ' a month' },
};
const giving = { plan: 'once', amount: 25, other: '' };

function donatePage() {
  const segs = Object.entries(PLANS).map(([id, plan]) => [id, el('button', { class: 'seg' }, plan.label)]);
  const presets = [0, 1, 2, 3, 4].map(() => el('button', { class: 'amount' }));
  const other = el('input', { type: 'text', inputmode: 'numeric', placeholder: 'Other', 'aria-label': 'Other amount' });
  other.value = giving.other;
  const paypal = el('a', { class: 'btn big donate-btn', target: '_blank', rel: 'noopener' });
  const sponsors = el('a', { class: 'btn big', target: '_blank', rel: 'noopener' });
  const note = el('p', { class: 'soft' });
  const update = () => {
    const plan = PLANS[giving.plan];
    const n = giving.other ? Number(giving.other) : giving.amount;
    segs.forEach(([id, b]) => b.classList.toggle('on', id === giving.plan));
    presets.forEach((b, i) => {
      fill(b, `$${plan.amounts[i]}`);
      b.classList.toggle('on', !giving.other && plan.amounts[i] === n);
    });
    const shown = n ? ` $${n}${plan.per}` : '';
    fill(paypal, icon(ICON.heart), `Donate${shown} with PayPal`);
    fill(sponsors, icon(ICON.heart), `Donate${shown} with GitHub Sponsors`);
    paypal.classList.toggle('hidden', giving.plan === 'month');
    paypal.href = n ? `${PAYPAL}/${n}USD` : PAYPAL;
    const query = { frequency: giving.plan === 'month' ? 'recurring' : 'one-time' };
    if (n) query.amount = n;
    sponsors.href = `${SPONSORS}/sponsorships?${new URLSearchParams(query)}`;
    fill(note, { once: '', month: 'Monthly gifts go through GitHub Sponsors.' }[giving.plan]);
  };
  segs.forEach(([id, b]) => {
    b.onclick = () => {
      const i = PLANS[giving.plan].amounts.indexOf(giving.amount);
      giving.plan = id;
      giving.amount = PLANS[id].amounts[i < 0 ? 2 : i];
      update();
    };
  });
  presets.forEach((b, i) => {
    b.onclick = () => { giving.amount = PLANS[giving.plan].amounts[i]; giving.other = ''; other.value = ''; update(); };
  });
  other.oninput = () => {
    other.value = other.value.replace(/[^0-9]/g, '').replace(/^0+/, '').slice(0, 5);
    giving.other = other.value;
    update();
  };
  update();
  return el('div', { class: 'donate' },
    el('div', { class: 'donate-heart' }, icon(ICON.heart)),
    el('h1', { class: 'title' }, 'Donate to PlayLibre'),
    el('p', { class: 'note' }, 'PlayLibre is free and open source. Donations are highly appreciated.'),
    el('div', { class: 'segs' }, segs.map(([, b]) => b)),
    el('div', { class: 'amounts' }, presets, el('label', { class: 'other' }, el('span', {}, '$'), other)),
    el('div', { class: 'givers' }, paypal, sponsors, note),
  );
}

// --- Credits -----------------------------------------------------------------

const buying = { packs: null, busy: false, error: '', paid: false };

function loadPacks() {
  if (buying.packs) return;
  buying.packs = { loading: true };
  api('/credits/packs')
    .then((p) => { buying.packs = p; })
    .catch((e) => { buying.packs = { error: e.message }; })
    .finally(() => render());
}

async function checkout(pack) {
  buying.busy = true;
  buying.error = '';
  render();
  const time = walletTime();
  const signature = signText(secretKey(), `libroxel checkout\n${session.playerId}\n${pack.id}\n${time}`);
  try {
    const { url } = await api('/credits/checkout', { method: 'POST', body: { player_id: session.playerId, pack: pack.id, time, signature } });
    location.href = url;
  } catch (e) {
    buying.busy = false;
    buying.error = e.message;
    render();
  }
}

function creditsPage() {
  if (!session) return needSignIn('Sign in to buy Credits.');
  loadPacks();
  if (new URLSearchParams(location.search).has('paid') && !buying.paid) {
    buying.paid = true;
    history.replaceState(null, '', '/credits');
    [0, 3000, 10000].forEach((ms) => setTimeout(() => { walletInfo = null; render(); }, ms));
  }
  const p = buying.packs;
  const money = (cents) => `$${(cents / 100).toFixed(2)}`;
  return el('div', { class: 'donate' },
    el('h1', { class: 'title' }, 'Buy Credits'),
    walletBar(),
    buying.paid && status('Thanks! Your Credits are on the way.'),
    buying.error && status(buying.error, 'error'),
    !p || p.loading ? el('p', { class: 'soft' }, 'Loading…')
      : p.error ? status(p.error, 'error')
        : !p.on ? el('p', { class: 'soft' }, 'Not available yet.')
          : el('div', { class: 'amounts' }, p.packs.map((pack) => el('button', { class: 'amount pack', disabled: buying.busy, onclick: () => checkout(pack) },
            el('span', {}, icon(BADGE_GLYPHS.coin), ` ${pack.credits.toLocaleString()}`),
            el('small', {}, money(pack.cents))))),
  );
}

// --- Privacy -----------------------------------------------------------------

const CONTACT = 'privacy@playlibre.net';

function privacy() {
  const h = (text) => el('h2', { class: 'section' }, text);
  const p = (...parts) => el('p', {}, ...parts);
  const list = (...items) => el('ul', {}, items.map((i) => el('li', {}, i)));
  const mail = el('a', { href: `mailto:${CONTACT}` }, CONTACT);
  return el('div', { class: 'privacy' },
    el('h1', { class: 'title' }, 'Privacy policy'),
    p(el('span', { class: 'soft' }, 'Last updated 23 November 2026')),
    p('This policy covers the PlayLibre app, this website and the PlayLibre Hub (hub.playlibre.net), which keeps accounts, friends, games and server lists.'),
    h('What we keep'),
    list(
      'Your account: your name, player number, avatar (colours and hat), the few words about yourself, and when the account was made.',
      'Your password never leaves your device. The Hub keeps only a key made from it and your account key locked with it, so we cannot read your password.',
      'Your email address, if you add one, and whether you proved it. We use it only to send codes and messages about your account.',
      'Two-step sign-in, if you turn it on: the secret your code app uses.',
      'Your friends and friend requests.',
      'Whether you are online, which game and server you are in, and when you were last online. Your friends see this.',
      'The time and IP address of your last 20 sign-ins, which your account page shows you so you can spot sign-ins that were not you.',
      'Games and hats you publish, servers you host (their name, game, join code and player count) and anything you report.',
      'When and how long you play each published game, for 90 days, under a code that can\'t be tied to your account. Its creator sees only totals.'),
    p('Unless you turn off Share in the app, the app also sends the Hub: how often it is used, under a random code made for this install and not tied to your account, with its version and system, kept for 90 days; and, when it crashes, what went wrong, with emails, passwords, keys and your user folder taken out, kept for 90 days. Only whoever runs the Hub sees these, as totals.'),
    p('Your name, number, avatar, about and published games are public: anyone can see them in the app and on this website.'),
    h('What we don\'t do'),
    list(
      'No ads and no tracking.',
      'No cookies. This website keeps your signed-in key in your browser\'s storage until you sign out or close the tab.',
      'We don\'t sell or share your information. The only others who handle it do so for us: the server that hosts the Hub and website, and the mail service that sends account emails.'),
    p('To stop abuse, the Hub counts requests from each IP address for a short time, in memory only. The Download page asks GitHub for the newest release, and downloads come from GitHub, under GitHub\'s own privacy policy.'),
    h('How long we keep it'),
    p('We keep your information while your account exists. Nightly backups are kept for 14 days. Servers drop off the list a few minutes after they stop.'),
    h('Your choices'),
    p('You can change your name, avatar, about, email and password at any time on the Avatar and Profile pages or in the app. To get a copy of your information, or to have your account and everything in it deleted, email ', mail, ' from the address on your account, or tell us your name and player number. We answer within 30 days.'),
    h('Children'),
    p('PlayLibre is not meant for children under 13. If you believe a child under 13 has an account, email ', mail.cloneNode(true), ' and we will delete it.'),
    h('Changes'),
    p('When this policy changes, the new one goes on this page with a new date.'),
    h('Contact'),
    p(mail.cloneNode(true)));
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

if (location.hash.startsWith('#/')) history.replaceState(null, '', location.hash.slice(1) + location.search);
document.addEventListener('click', (e) => {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const a = e.target.closest('a[href]');
  if (!a || a.target || a.hasAttribute('download') || a.origin !== location.origin) return;
  const path = a.getAttribute('href');
  if (!path.startsWith('/') || path.startsWith('/downloads/')) return;
  e.preventDefault();
  goTo(a.pathname + a.search);
});
window.addEventListener('popstate', render);
function sayOnline() {
  if (!session || document.hidden) return;
  const time = Date.now();
  const signature = signText(secretKey(), `libroxel web\n${session.playerId}\n${time}`);
  api('/presence/web', { method: 'POST', body: { player_id: session.playerId, time, signature } }).catch(() => {});
}

render();
refresh().then(render);
loadChats().then(redrawChats);
setInterval(() => {
  if (session && !document.hidden && (route.name === 'messages' || Date.now() - chats.at > 15000)) loadChats().then(redrawChats);
}, 4000);
sayOnline();
setInterval(sayOnline, 15000);
setInterval(() => {
  if (document.hidden || !session || route.name !== 'creator' || route.tab !== 'data' || !saved.game) return;
  if (saved.editing !== null || saved.adding || saved.busy) return;
  if (document.activeElement && ['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)) return;
  loadSaved(saved.game, true);
}, 3000);
document.addEventListener('visibilitychange', sayOnline);
setInterval(() => {
  // Keep lists fresh while someone looks, but don't redraw under their typing.
  if (document.hidden || ['sign-in', 'create', 'avatar', 'creator', 'profile'].includes(route.name)) return;
  if (document.activeElement && ['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)) return;
  refresh().then(render);
}, 20000);

