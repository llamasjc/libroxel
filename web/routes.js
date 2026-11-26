// A game's page is games: /games/<number>/<name>, such as
// /games/12/my-cool-game. The number alone finds it, and a wrong or old name
// moves to the right one. Older links by a game's id (game: /game/<id>) move
// to its numbered page too.
export const ROUTES = {
  home: { title: 'Home', params: [] },
  games: { title: 'Game', params: ['number', 'slug'] },
  game: { title: 'Game', params: ['key'], rest: true },
  user: { title: 'Player', params: ['id'] },
  creator: { title: 'Create', params: ['id', 'tab'] },
  market: { title: 'Marketplace', params: ['id'] },
  messages: { title: 'Messages', params: ['id'] },
  group: { title: 'Group', params: ['id'] },
  charts: { title: 'Charts', params: [] },
  profile: { title: 'Profile', params: [] },
  search: { title: 'Search', params: [] },
  friends: { title: 'Friends', params: [] },
  groups: { title: 'Groups', params: [] },
  avatar: { title: 'Avatar', params: [] },
  'sign-in': { title: 'Sign in', params: [] },
  create: { title: 'Make an account', params: [] },
  download: { title: 'Download', params: [] },
  password: { title: 'Password', params: [] },
  privacy: { title: 'Privacy policy', params: [] },
  donate: { title: 'Donate', params: [] },
  moderation: { title: 'Moderation', params: [] },
  stats: { title: 'Stats', params: [] },
};

export const ALIASES = {};

export function routeOf(pathname) {
  const parts = pathname.replace(/^\/+|\/+$/g, '').split('/').map((p) => {
    try {
      return decodeURIComponent(p);
    } catch {
      return p;
    }
  });
  // /games on its own is still the list of games.
  if (parts[0] === 'games' && !/^\d+$/.test(parts[1] || '')) return { name: 'charts' };
  const name = ALIASES[parts[0]] || parts[0];
  const route = ROUTES[name];
  if (!route || name === 'home') return { name: 'home' };
  const found = { name };
  route.params.forEach((param, i) => {
    found[param] = route.rest && i === route.params.length - 1 ? parts.slice(i + 1).join('/') : parts[i + 1];
  });
  return found;
}

export function href(name, ...parts) {
  if (!ROUTES[name]) throw new Error(`no route ${name}`);
  if (name === 'home') return '/';
  return `/${[name, ...parts.filter((p) => p !== undefined && p !== null && p !== '').map(encodeURIComponent)].join('/')}`;
}

/** The end of a game's page address: its title in small letters, with a dash
 * between words. The Hub makes the same one (`game_slug` in
 * crates/playlibre-hub/src/lib.rs). */
export function slug(title) {
  let out = '';
  for (const c of String(title || '').toLowerCase()) {
    if (/^[a-z0-9]$/.test(c)) out += c;
    else if (out && !out.endsWith('-')) out += '-';
  }
  out = out.replace(/-+$/, '');
  return out.length > 60 ? out.slice(0, 60).replace(/-+$/, '') : out;
}

/** A game's page from its number and title: /games/12/my-cool-game. */
export function gamePath(number, title) {
  const end = slug(title);
  return end ? `/games/${number}/${end}` : `/games/${number}`;
}
