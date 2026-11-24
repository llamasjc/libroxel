export const ROUTES = {
  home: { title: 'Home', params: [] },
  game: { title: 'Game', params: ['key'], rest: true },
  user: { title: 'Player', params: ['id'] },
  creator: { title: 'Create', params: ['id', 'tab'] },
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

export const ALIASES = { games: 'charts' };

export function routeOf(pathname) {
  const parts = pathname.replace(/^\/+|\/+$/g, '').split('/').map((p) => {
    try {
      return decodeURIComponent(p);
    } catch {
      return p;
    }
  });
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
