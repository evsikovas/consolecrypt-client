// SPDX-License-Identifier: AGPL-3.0-only
// Release operator: keys and installer bodies are never written to disk.
import { createHash, createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

export const REPOSITORY = 'evsikovas/consolecrypt-client';
export const REPOSITORY_ID = '1404811526';
export const TRUST_ANCHOR = 'aK3R5vXwJ9eeqQ1iYah9fzBAIHOeKNejoRdI8weXUYo=';
export const DAY = 86_400_000;
export const MAX_INSTALLER = 524_288_000;
const MAX_JSON = 1_048_576;
const MAX_FEED = 65_536;
const MAX_ASSETS = 32;
const MAX_RELEASES = 100;
const API = `https://api.github.com/repos/${REPOSITORY}`;
const DOWNLOAD = `https://github.com/${REPOSITORY}/releases/download/`;
const FEEDS = {
  'stable.json': 'updates.consolecrypt.dev',
  'legacy-stable.json': 'updates.consolecrypt.evsikov.net',
};
const SUFFIXES = {
  'windows-x64': 'windows-x64-setup.exe',
  'macos-universal': 'macos-universal.dmg',
  'android-arm64': 'android-arm64.apk',
};
const fail = message => { const error = new Error(message); error.safeForLog = true; throw error; };
const integer = (n, max = Number.MAX_SAFE_INTEGER) => Number.isSafeInteger(n) && n > 0 && n <= max;
const hexHash = s => typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);

export function versionParts(version) {
  if (typeof version !== 'string' || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) fail('Invalid stable version');
  const parts = version.split('.').map(Number);
  if (!parts.every(n => Number.isSafeInteger(n) && n >= 0)) fail('Invalid stable version');
  return parts;
}
export function compareVersions(a, b) {
  const left = versionParts(a), right = versionParts(b);
  for (let i = 0; i < 3; i++) if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  return 0;
}
function releaseVersion(release) {
  if (!release || !integer(release.id) || release.draft !== false || release.prerelease !== false || !release.published_at || typeof release.tag_name !== 'string' || !release.tag_name.startsWith('v')) fail('Invalid published stable release');
  const version = release.tag_name.slice(1);
  versionParts(version);
  return version;
}
function assetName(version, platform, build) {
  if (!integer(build, 2_100_000_000)) fail('Invalid installer build');
  return `ConsoleCrypt-${version}+${build}-${SUFFIXES[platform]}`;
}
function downloadUrl(tag, name) { return DOWNLOAD + tag + '/' + encodeURIComponent(name); }

export function validateDownloadUrl(value, tag, name) {
  let url;
  try { url = new URL(value); } catch { fail('Invalid asset download URL'); }
  if (url.protocol !== 'https:' || url.port || url.username || url.password || url.hash) fail('Unsafe asset download URL');
  if (url.hostname === 'github.com') {
    const prefix = `/${REPOSITORY}/releases/download/${tag}/`;
    let filename;
    try { filename = decodeURIComponent(url.pathname.slice(prefix.length)); } catch { fail('Invalid asset path'); }
    if (url.search || !url.pathname.startsWith(prefix) || filename !== name) fail('Foreign GitHub asset path');
  } else if (url.hostname === 'release-assets.githubusercontent.com') {
    const pattern = new RegExp(`^/github-production-release-asset/${REPOSITORY_ID}/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`);
    if (!pattern.test(url.pathname)) fail('Foreign release CDN path');
  } else fail('Foreign asset download host');
  return url;
}

async function boundedBody(response, limit, onChunk) {
  const length = response.headers.get('content-length');
  const encoded = response.headers.get('content-encoding');
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > limit)) {
    await response.body?.cancel();
    fail('Response exceeds size limit');
  }
  if (!response.body) fail('Missing response body');
  let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.length;
    if (bytes > limit) fail('Response exceeds size limit');
    onChunk(chunk);
  }
  // Fetch decodes HTTP content encodings before exposing chunks. In that case
  // Content-Length describes compressed bytes, while our bound covers decoded data.
  if (length !== null && (!encoded || encoded === 'identity') && bytes !== Number(length)) fail('Response length mismatch');
  return bytes;
}
async function responseBytes(response, limit) {
  const chunks = [];
  await boundedBody(response, limit, chunk => chunks.push(chunk));
  return Buffer.concat(chunks);
}

export async function downloadAsset(asset, tag, { fetchImpl = fetch, hashOnly = false, limit = MAX_FEED } = {}) {
  const expected = downloadUrl(tag, asset.name);
  if (asset.browser_download_url) {
    const metadataUrl = validateDownloadUrl(asset.browser_download_url, tag, asset.name);
    if (metadataUrl.hostname !== 'github.com') fail('Unexpected initial download URL');
  }
  let current = expected;
  for (let hop = 0; hop <= 3; hop++) {
    validateDownloadUrl(current, tag, asset.name);
    // Public downloads never receive the API token, even on github.com.
    const response = await fetchImpl(current, {
      redirect: 'manual', headers: { 'User-Agent': 'ConsoleCrypt-update-publisher', 'Accept-Encoding': 'identity' },
      signal: AbortSignal.timeout(10 * 60_000),
    });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get('location');
      await response.body?.cancel();
      if (!location || hop === 3) fail('Invalid download redirect');
      current = new URL(location, current).href;
      continue;
    }
    if (response.status !== 200) fail(`Public asset download failed (${response.status})`);
    if (hashOnly) {
      const hash = createHash('sha256');
      const bytes = await boundedBody(response, limit, chunk => hash.update(chunk));
      if (bytes !== asset.size) fail('Installer size mismatch');
      return { bytes, sha256: hash.digest('hex') };
    }
    const body = await responseBytes(response, limit);
    if (body.length !== asset.size) fail('Asset size mismatch');
    return body;
  }
  fail('Too many download redirects');
}

export function parseChecksums(text, version) {
  if (Buffer.byteLength(text) > MAX_FEED) fail('Checksums exceed size limit');
  const lines = text.trim().split(/\r?\n/);
  if (!lines.length || lines.length > MAX_ASSETS) fail('Invalid checksum count');
  const sums = new Map();
  for (const line of lines) {
    const match = /^([0-9a-f]{64}) [ *](ConsoleCrypt-[A-Za-z0-9.+_-]{1,180})$/.exec(line);
    if (!match || !match[2].startsWith(`ConsoleCrypt-${version}+`) || sums.has(match[2])) fail('Invalid or duplicate checksum');
    sums.set(match[2], match[1]);
  }
  return sums;
}
function validateAssets(assets) {
  if (!Array.isArray(assets) || !assets.length || assets.length > MAX_ASSETS) fail('Invalid release asset count');
  const names = new Set(), ids = new Set();
  for (const asset of assets) {
    if (!integer(asset.id) || ids.has(asset.id) || typeof asset.name !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9.+_-]{0,200}$/.test(asset.name) || names.has(asset.name) || asset.state !== 'uploaded' || !integer(asset.size, 2_147_483_648)) fail('Invalid release asset metadata');
    names.add(asset.name); ids.add(asset.id);
  }
  return assets;
}
export function selectInstallers(assets, sums, version) {
  validateAssets(assets);
  const selected = {};
  for (const [platform, suffix] of Object.entries(SUFFIXES)) {
    const candidates = assets.filter(asset => asset.name.endsWith(`-${suffix}`));
    if (candidates.length !== 1) fail('Missing or ambiguous installer');
    const asset = candidates[0];
    const prefix = `ConsoleCrypt-${version}+`;
    const buildText = asset.name.slice(prefix.length, -suffix.length - 1);
    if (!asset.name.startsWith(prefix) || !/^[1-9]\d*$/.test(buildText)) fail('Invalid installer identity');
    const build = Number(buildText);
    if (asset.name !== assetName(version, platform, build) || asset.size > MAX_INSTALLER || !sums.has(asset.name)) fail('Invalid installer identity');
    validateDownloadUrl(asset.browser_download_url, `v${version}`, asset.name);
    if (asset.digest != null && asset.digest !== `sha256:${sums.get(asset.name)}`) fail('GitHub asset digest mismatch');
    selected[platform] = { ...asset, build, sha256: sums.get(asset.name) };
  }
  for (const name of sums.keys()) if (!assets.some(asset => asset.name === name)) fail('Checksum names an absent release asset');
  return selected;
}

function rawPublicKey(anchor) {
  const bytes = Buffer.from(anchor, 'base64');
  if (bytes.length !== 32 || bytes.toString('base64') !== anchor) fail('Invalid trust anchor');
  return createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), bytes]), format: 'der', type: 'spki' });
}
function decode64(text, limit) {
  if (typeof text !== 'string' || text.length > limit * 2) fail('Invalid signed envelope');
  const bytes = Buffer.from(text, 'base64');
  if (bytes.length > limit || bytes.toString('base64') !== text) fail('Invalid signed envelope');
  return bytes;
}
export function verifyFeed(body, host, { anchor = TRUST_ANCHOR, now = new Date() } = {}) {
  if (Buffer.byteLength(body) > MAX_FEED || !Object.values(FEEDS).includes(host)) fail('Invalid existing feed');
  const wrapper = JSON.parse(body.toString());
  const bytes = decode64(wrapper.payload, 32_768), signature = decode64(wrapper.signature, 64);
  if (signature.length !== 64 || !verify(null, bytes, rawPublicKey(anchor), signature)) fail('Existing feed signature does not match trust anchor');
  const payload = JSON.parse(bytes.toString());
  versionParts(payload.version);
  const issued = Date.parse(payload.issuedAt), expires = Date.parse(payload.expiresAt);
  if (payload.schema !== 1 || !Number.isFinite(issued) || !Number.isFinite(expires) || issued > now.getTime() + 600_000 || expires <= issued || expires - issued > 90 * DAY) fail('Invalid existing feed lifetime');
  if (!payload.platforms || Object.keys(payload.platforms).sort().join() !== Object.keys(SUFFIXES).sort().join()) fail('Invalid existing feed platforms');
  for (const [platform, item] of Object.entries(payload.platforms)) {
    const name = assetName(payload.version, platform, item.build);
    if (!integer(item.bytes, MAX_INSTALLER) || !hexHash(item.sha256) || item.url !== `https://${host}/releases/${payload.version}/${encodeURIComponent(name)}`) fail('Invalid existing feed identity');
  }
  if (!payload.notes || !['ru', 'en'].every(locale => typeof payload.notes[locale] === 'string' && payload.notes[locale].length <= 4000)) fail('Invalid feed notes');
  return payload; // Expired, correctly signed baselines can be renewed.
}
function coreIdentity(feed) {
  return JSON.stringify([feed.version, ...Object.keys(SUFFIXES).map(platform => {
    const item = feed.platforms[platform];
    return [platform, item.build, item.bytes, item.sha256];
  })]);
}
export function assertNoDowngrade(previous, version, selected) {
  const order = compareVersions(version, previous.version);
  if (order < 0) fail('Release downgrade refused');
  for (const [platform, asset] of Object.entries(selected)) {
    const old = previous.platforms[platform];
    if (order === 0 && (old.build !== asset.build || old.bytes !== asset.size || old.sha256 !== asset.sha256)) fail('Published release identity changed');
    if (order > 0 && asset.build <= old.build) fail('Installer build downgrade refused');
  }
}
function signingKey(text, anchor) {
  const key = createPrivateKey(text);
  if (key.asymmetricKeyType !== 'ed25519' || createPublicKey(key).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64') !== anchor) fail('Update signing key does not match existing clients');
  return key;
}
export function signFeeds(version, selected, key, now, notes) {
  const feeds = {};
  for (const [name, host] of Object.entries(FEEDS)) {
    const platforms = Object.fromEntries(Object.entries(selected).map(([platform, asset]) => [platform, {
      url: `https://${host}/releases/${version}/${encodeURIComponent(asset.name)}`,
      build: asset.build, bytes: asset.size, sha256: asset.sha256,
    }]));
    const payload = Buffer.from(JSON.stringify({ schema: 1, version, issuedAt: now.toISOString(), expiresAt: new Date(now.getTime() + 90 * DAY).toISOString(), notes, platforms }));
    feeds[name] = Buffer.from(JSON.stringify({ payload: payload.toString('base64'), signature: sign(null, payload, key).toString('base64') }) + '\n');
  }
  return feeds;
}

export async function publishUpdateFeeds({ token, keyText, fetchImpl = fetch, now = new Date(), anchor = TRUST_ANCHOR } = {}) {
  if (!token || !keyText) fail('GITHUB_TOKEN and UPDATE_SIGNING_KEY are required');
  const key = signingKey(keyText, anchor);
  async function api(path, { method = 'GET', body, upload = false } = {}) {
    const base = upload ? `https://uploads.github.com/repos/${REPOSITORY}` : API;
    const response = await fetchImpl(base + path, {
      method, redirect: 'manual', signal: AbortSignal.timeout(60_000),
      headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'Accept-Encoding': 'identity', 'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'ConsoleCrypt-update-publisher', ...(body ? { 'Content-Type': 'application/json' } : {}) },
      ...(body ? { body: Buffer.isBuffer(body) ? body : JSON.stringify(body) } : {}),
    });
    if (response.status === 204) return null;
    if (![200, 201].includes(response.status)) { await response.body?.cancel(); fail(`GitHub API ${method} failed (${response.status})`); }
    return JSON.parse((await responseBytes(response, MAX_JSON)).toString());
  }
  const latest = await api('/releases/latest');
  const version = releaseVersion(latest);
  if (latest.immutable === true) fail('Immutable release assets cannot renew feeds; operator review required');
  const history = await api(`/releases?per_page=${MAX_RELEASES}`);
  if (!Array.isArray(history) || history.length >= MAX_RELEASES) fail('Release history requires operator review');
  const stable = history.filter(release => !release.draft && !release.prerelease && /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(release.tag_name));
  stable.sort((a, b) => compareVersions(releaseVersion(b), releaseVersion(a)));
  if (!stable.length || stable[0].id !== latest.id || compareVersions(version, releaseVersion(stable[0])) !== 0) fail('Latest release is not the highest stable version');
  const latestAssets = validateAssets(await api(`/releases/${latest.id}/assets?per_page=100`));
  const checksum = latestAssets.find(asset => asset.name === `SHA256SUMS-${version}.txt`);
  if (!checksum || checksum.size > MAX_FEED) fail('Missing release checksums');
  const sums = parseChecksums((await downloadAsset(checksum, latest.tag_name, { fetchImpl })).toString(), version);
  const selected = selectInstallers(latestAssets, sums, version);
  let previous;
  let currentFeeds = {};
  for (const release of stable) {
    const assets = release.id === latest.id ? latestAssets : validateAssets(await api(`/releases/${release.id}/assets?per_page=100`));
    const found = {};
    for (const [name, host] of Object.entries(FEEDS)) {
      const asset = assets.find(item => item.name === name);
      if (!asset) continue;
      const body = await downloadAsset(asset, release.tag_name, { fetchImpl });
      const payload = verifyFeed(body, host, { anchor, now });
      if (payload.version !== releaseVersion(release)) fail('Feed and release version mismatch');
      found[name] = { asset, body, payload };
    }
    if (!Object.keys(found).length) continue;
    const payloads = Object.values(found).map(item => item.payload);
    if (payloads.some(payload => coreIdentity(payload) !== coreIdentity(payloads[0]))) fail('Existing feeds disagree about installer identities');
    previous = payloads[0];
    if (release.id === latest.id) currentFeeds = found;
    break;
  }
  if (!previous) fail('Bootstrap requires an existing feed signed by the client trust anchor');
  assertNoDowngrade(previous, version, selected);
  const complete = Object.keys(currentFeeds).length === 2;
  if (complete && Object.values(currentFeeds).every(feed => Date.parse(feed.payload.expiresAt) - now.getTime() >= 14 * DAY)) {
    return { status: 'unchanged', version }; // Do not redownload GBs or re-sign daily.
  }
  for (const asset of Object.values(selected)) {
    const measured = await downloadAsset(asset, latest.tag_name, { fetchImpl, hashOnly: true, limit: MAX_INSTALLER });
    if (measured.sha256 !== asset.sha256) fail('Installer checksum mismatch');
  }
  // Fail before writes if a new release or edited metadata appeared while hashing.
  const recheck = await api('/releases/latest');
  if (recheck.id !== latest.id || recheck.tag_name !== latest.tag_name || recheck.draft || recheck.prerelease) fail('Latest release changed during verification');
  const afterAssets = validateAssets(await api(`/releases/${latest.id}/assets?per_page=100`));
  if (JSON.stringify(afterAssets.map(a => [a.id, a.name, a.size, a.digest, a.updated_at]).sort()) !== JSON.stringify(latestAssets.map(a => [a.id, a.name, a.size, a.digest, a.updated_at]).sort())) fail('Release assets changed during verification');
  const notes = previous.version === version ? previous.notes : {
    ru: `Обновление ConsoleCrypt ${version}. Описание: https://github.com/${REPOSITORY}/releases/tag/v${version}`,
    en: `ConsoleCrypt ${version} update. Release notes: https://github.com/${REPOSITORY}/releases/tag/v${version}`,
  };
  const feeds = signFeeds(version, selected, key, now, notes);
  async function uploadFeed(name, body) {
    const uploaded = await api(`/releases/${latest.id}/assets?name=${encodeURIComponent(name)}`, { method: 'POST', body, upload: true });
    validateAssets([uploaded]);
    if (uploaded.name !== name || uploaded.size !== body.length || (uploaded.digest != null && uploaded.digest !== `sha256:${createHash('sha256').update(body).digest('hex')}`)) fail('Uploaded feed metadata mismatch');
    const location = validateDownloadUrl(uploaded.browser_download_url, latest.tag_name, name);
    if (location.hostname !== 'github.com') fail('Uploaded feed has an unexpected location');
  }
  for (const [name, body] of Object.entries(feeds)) {
    verifyFeed(body, FEEDS[name], { anchor, now });
    const old = currentFeeds[name];
    if (old) await api(`/releases/assets/${old.asset.id}`, { method: 'DELETE' });
    try {
      await uploadFeed(name, body);
    } catch (error) {
      // GitHub has no atomic replace. Best effort restores the previous public
      // feed; the serving mirror must retain its last verified pair on failure.
      if (old) {
        try { await uploadFeed(name, old.body); } catch { /* Next run repairs from the other signed feed or operator backup. */ }
      }
      throw error;
    }
  }
  return { status: 'published', version, expiresAt: new Date(now.getTime() + 90 * DAY).toISOString(), installers: 3 };
}

async function main() {
  if (process.env.GITHUB_ACTIONS !== 'true' || process.env.GITHUB_REPOSITORY !== REPOSITORY || process.env.GITHUB_REF !== 'refs/heads/main' || !['schedule', 'workflow_dispatch'].includes(process.env.GITHUB_EVENT_NAME)) fail('Run only from the trusted main GitHub workflow');
  const token = process.env.GITHUB_TOKEN;
  const keyText = process.env.UPDATE_SIGNING_KEY;
  delete process.env.UPDATE_SIGNING_KEY;
  delete process.env.GITHUB_TOKEN;
  const result = await publishUpdateFeeds({ token, keyText });
  console.log(JSON.stringify(result));
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => {
    console.error(error.safeForLog ? error.message : 'Update feed publication failed; check connectivity, release metadata and signing-key format.');
    process.exitCode = 1;
  });
}
