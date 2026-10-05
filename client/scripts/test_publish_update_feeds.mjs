// SPDX-License-Identifier: AGPL-3.0-only
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto';
import {
  DAY, MAX_INSTALLER, REPOSITORY, REPOSITORY_ID, assertNoDowngrade,
  compareVersions, downloadAsset, parseChecksums, publishUpdateFeeds,
  selectInstallers, signFeeds, validateDownloadUrl, verifyFeed,
} from './publish-update-feeds.mjs';

const NOW = new Date('2026-10-06T10:00:00.000Z');
const HOST = 'updates.consolecrypt.dev';
const LEGACY = 'updates.consolecrypt.evsikov.net';
const API = `https://api.github.com/repos/${REPOSITORY}`;
const SUFFIXES = { 'windows-x64': 'windows-x64-setup.exe', 'macos-universal': 'macos-universal.dmg', 'android-arm64': 'android-arm64.apk' };
const hash = body => createHash('sha256').update(body).digest('hex');
const json = value => new Response(JSON.stringify(value), { status: 200 });
function makeKeys() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return {
    privateKey, keyText: privateKey.export({ type: 'pkcs8', format: 'pem' }),
    anchor: publicKey.export({ type: 'spki', format: 'der' }).subarray(-32).toString('base64'),
  };
}
function fixture({ sameVersion = false, age = 1, corruptInstaller = false, redirectHost, failUpload = false, mutateAssets = false, bootstrap = true } = {}) {
  const keys = makeKeys(), token = randomBytes(24).toString('hex');
  const blobs = new Map(), downloads = [], writes = [], calls = [];
  let nextId = 10, latestReads = 0, failedUpload = false;
  const version = sameVersion ? '0.3.1' : '0.3.2';
  function release(id, ver) { return { id, tag_name: `v${ver}`, draft: false, prerelease: false, published_at: NOW.toISOString(), assets: [] }; }
  const previous = release(1, '0.3.1'), target = sameVersion ? previous : release(2, version);
  function add(release, name, body) {
    const asset = { id: nextId++, name, size: body.length, state: 'uploaded', updated_at: '2026-10-05T00:00:00Z', digest: `sha256:${hash(body)}`, browser_download_url: `https://github.com/${REPOSITORY}/releases/download/${release.tag_name}/${encodeURIComponent(name)}` };
    release.assets.push(asset); blobs.set(`${release.tag_name}/${name}`, Buffer.from(body)); return asset;
  }
  function packages(release, baseBuild) {
    const selected = {}, ver = release.tag_name.slice(1);
    for (const [index, [platform, suffix]] of Object.entries(SUFFIXES).entries()) {
      const build = baseBuild + index;
      const body = Buffer.from(`Synthetic package ${platform} ${ver}, not an executable.`);
      const asset = add(release, `ConsoleCrypt-${ver}+${build}-${suffix}`, body);
      selected[platform] = { ...asset, build, sha256: hash(body) };
    }
    add(release, `SHA256SUMS-${ver}.txt`, Buffer.from(Object.values(selected).map(asset => `${asset.sha256}  ${asset.name}`).join('\n') + '\n'));
    return selected;
  }
  const oldSelected = packages(previous, 1390);
  const selected = sameVersion ? oldSelected : packages(target, 13010);
  const notes = { ru: 'Проверенный выпуск', en: 'Verified release' };
  const oldFeeds = signFeeds('0.3.1', oldSelected, keys.privateKey, new Date(NOW.getTime() - age * DAY), notes);
  if (bootstrap) for (const [name, body] of Object.entries(oldFeeds)) add(previous, name, body);
  const history = sameVersion ? [target] : [target, previous];
  const cdnBodies = new Map();
  async function fetchImpl(value, options = {}) {
    const url = new URL(value), method = options.method ?? 'GET';
    calls.push({ origin: url.origin, path: url.pathname, method });
    if (url.hostname === 'api.github.com' || url.hostname === 'uploads.github.com') {
      assert.equal(options.headers.Authorization, `Bearer ${token}`);
      assert.equal(options.redirect, 'manual');
      const path = url.pathname.slice(`/repos/${REPOSITORY}`.length);
      if (method === 'GET' && path === '/releases/latest') {
        latestReads++;
        if (mutateAssets && latestReads === 2) target.assets[0].updated_at = '2026-10-06T10:01:00Z';
        return json(target);
      }
      if (method === 'GET' && path === '/releases') return json(history);
      const list = /^\/releases\/(\d+)\/assets$/.exec(path);
      if (method === 'GET' && list) return json(history.find(release => release.id === +list[1]).assets);
      const remove = /^\/releases\/assets\/(\d+)$/.exec(path);
      if (method === 'DELETE' && remove) {
        const index = target.assets.findIndex(asset => asset.id === +remove[1]);
        assert(index >= 0);
        const [asset] = target.assets.splice(index, 1);
        writes.push({ method, name: asset.name });
        blobs.delete(`${target.tag_name}/${asset.name}`);
        return new Response(null, { status: 204 });
      }
      if (method === 'POST' && list && url.hostname === 'uploads.github.com') {
        const name = url.searchParams.get('name'), body = Buffer.from(options.body);
        writes.push({ method, name, body });
        if (failUpload && !failedUpload) { failedUpload = true; return new Response(null, { status: 500 }); }
        assert(!target.assets.some(asset => asset.name === name));
        return new Response(JSON.stringify(add(target, name, body)), { status: 201 });
      }
      throw Error('Unexpected API route in test');
    }
    assert.equal(options.headers.Authorization, undefined, 'Never forward the API token to a public download');
    assert.equal(options.redirect, 'manual');
    if (url.hostname === 'github.com') {
      const key = decodeURIComponent(url.pathname.split('/releases/download/')[1]);
      const body = blobs.get(key);
      assert(body, `Unknown fixture asset ${key}`);
      const uuid = `00000000-0000-0000-0000-${String(cdnBodies.size + 1).padStart(12, '0')}`;
      const path = `/github-production-release-asset/${REPOSITORY_ID}/${uuid}`;
      cdnBodies.set(path, { body, key });
      return new Response(null, { status: 302, headers: { location: `https://${redirectHost ?? 'release-assets.githubusercontent.com'}${path}?signed=ephemeral` } });
    }
    if (url.hostname === 'release-assets.githubusercontent.com') {
      const { body, key } = cdnBodies.get(url.pathname);
      downloads.push(key);
      const installer = Object.values(SUFFIXES).some(suffix => key.endsWith(suffix));
      const bytes = Buffer.from(body);
      if (corruptInstaller && installer) bytes[0] ^= 1;
      return new Response(bytes, { status: 200, headers: { 'content-length': String(bytes.length) } });
    }
    throw Error('Unexpected public host reached in test');
  }
  return { ...keys, token, fetchImpl, now: NOW, target, previous, selected, oldSelected, history, blobs, oldFeeds, downloads, writes, calls, add };
}

test('new stable release streams three installers and signs both domains with the existing anchor', async () => {
  const f = fixture();
  const result = await publishUpdateFeeds(f);
  assert.equal(result.status, 'published'); assert.equal(result.installers, 3);
  assert.equal(f.downloads.filter(name => Object.values(SUFFIXES).some(suffix => name.endsWith(suffix))).length, 3);
  assert.equal(f.writes.length, 2);
  for (const [name, host] of [['stable.json', HOST], ['legacy-stable.json', LEGACY]]) {
    const feed = verifyFeed(f.blobs.get(`v0.3.2/${name}`), host, { anchor: f.anchor, now: NOW });
    assert.equal(feed.version, '0.3.2');
    assert.equal(Date.parse(feed.expiresAt) - Date.parse(feed.issuedAt), 90 * DAY);
    assert.equal(feed.platforms['macos-universal'].sha256, f.selected['macos-universal'].sha256);
    assert(feed.platforms['macos-universal'].url.startsWith(`https://${host}/releases/0.3.2/`));
  }
});

test('fresh feeds, including exactly fourteen days remaining, are unchanged without installer downloads', async () => {
  for (const age of [1, 76]) {
    const f = fixture({ sameVersion: true, age });
    assert.equal((await publishUpdateFeeds(f)).status, 'unchanged');
    assert.equal(f.writes.length, 0);
    assert(!f.downloads.some(name => Object.values(SUFFIXES).some(suffix => name.endsWith(suffix))));
  }
});

test('near-expiry and expired correctly signed feeds renew for at most ninety days', async () => {
  for (const age of [77, 100]) {
    const f = fixture({ sameVersion: true, age });
    assert.equal((await publishUpdateFeeds(f)).status, 'published');
    const feed = verifyFeed(f.blobs.get('v0.3.1/stable.json'), HOST, { anchor: f.anchor, now: NOW });
    assert.equal(feed.notes.en, 'Verified release');
    assert.equal(feed.issuedAt, NOW.toISOString());
    assert.equal(f.writes.filter(write => write.method === 'POST').length, 2);
  }
});

test('an interrupted first publication with only one valid feed can recover the pair', async () => {
  const f = fixture({ sameVersion: true });
  f.target.assets = f.target.assets.filter(asset => asset.name !== 'legacy-stable.json');
  assert.equal((await publishUpdateFeeds(f)).status, 'published');
  assert(f.blobs.has('v0.3.1/legacy-stable.json'));
});

test('wrong signer, untrusted baseline and tampered installer all fail before writes', async () => {
  const wrong = fixture();
  wrong.keyText = makeKeys().keyText;
  await assert.rejects(publishUpdateFeeds(wrong), /signing key does not match/);
  assert.equal(wrong.calls.length, 0);
  const missing = fixture({ bootstrap: false });
  await assert.rejects(publishUpdateFeeds(missing), /Bootstrap requires/);
  assert.equal(missing.writes.length, 0);
  const corrupt = fixture({ corruptInstaller: true });
  await assert.rejects(publishUpdateFeeds(corrupt), /Installer checksum mismatch/);
  assert.equal(corrupt.writes.length, 0);
});

test('existing feed signature must verify before its version or URLs are trusted', async () => {
  const f = fixture();
  const wrapper = JSON.parse(f.oldFeeds['stable.json']);
  const payload = JSON.parse(Buffer.from(wrapper.payload, 'base64'));
  payload.version = '0.0.1';
  wrapper.payload = Buffer.from(JSON.stringify(payload)).toString('base64');
  const altered = Buffer.from(JSON.stringify(wrapper) + '\n');
  f.blobs.set('v0.3.1/stable.json', altered);
  f.previous.assets.find(asset => asset.name === 'stable.json').size = altered.length;
  await assert.rejects(publishUpdateFeeds(f), /signature does not match/);
  assert.equal(f.writes.length, 0);
});

test('malformed, duplicate, incomplete and oversized installers are rejected', () => {
  const f = fixture();
  const sums = parseChecksums(f.blobs.get('v0.3.2/SHA256SUMS-0.3.2.txt').toString(), '0.3.2');
  assert.throws(() => selectInstallers(f.target.assets.slice(1), sums, '0.3.2'), /Missing or ambiguous/);
  const duplicate = { ...f.target.assets[0], id: 999, name: f.target.assets[0].name.replace('+13010-', '+13020-') };
  assert.throws(() => selectInstallers([...f.target.assets, duplicate], sums, '0.3.2'), /Missing or ambiguous/);
  const oversized = structuredClone(f.target.assets); oversized[0].size = MAX_INSTALLER + 1;
  assert.throws(() => selectInstallers(oversized, sums, '0.3.2'), /Invalid installer identity/);
  const badBuild = structuredClone(f.target.assets); badBuild[0].name = badBuild[0].name.replace('+13010-', '+013010-');
  assert.throws(() => selectInstallers(badBuild, sums, '0.3.2'), /Invalid installer identity/);
  const mismatchedDigest = structuredClone(f.target.assets); mismatchedDigest[0].digest = 'sha256:' + '0'.repeat(64);
  assert.throws(() => selectInstallers(mismatchedDigest, sums, '0.3.2'), /digest mismatch/);
});

test('checksum parsing rejects duplicate names, traversal and a different release', () => {
  const f = fixture();
  const text = f.blobs.get('v0.3.2/SHA256SUMS-0.3.2.txt').toString();
  assert.throws(() => parseChecksums(text + text, '0.3.2'), /duplicate checksum/);
  assert.throws(() => parseChecksums('0'.repeat(64) + '  ../key.pem', '0.3.2'), /Invalid/);
  assert.throws(() => parseChecksums(text, '0.3.3'), /Invalid/);
});

test('download URLs permit only this exact GitHub release path or repository CDN object', async () => {
  const name = 'ConsoleCrypt-0.3.2+13011-macos-universal.dmg';
  const good = `https://github.com/${REPOSITORY}/releases/download/v0.3.2/${encodeURIComponent(name)}`;
  assert.equal(validateDownloadUrl(good, 'v0.3.2', name).hostname, 'github.com');
  for (const url of [good.replace('https:', 'http:'), good.replace('github.com/', 'github.com.evil.test/'), good.replace(REPOSITORY, 'other/repo'), good + '?download=1', good + '#fragment', good.replace('github.com', 'user:pass@github.com'), good.replace('github.com', 'github.com:444'), 'https://release-assets.githubusercontent.com/github-production-release-asset/1/00000000-0000-0000-0000-000000000001']) {
    assert.throws(() => validateDownloadUrl(url, 'v0.3.2', name));
  }
  const f = fixture({ redirectHost: 'attacker.example' });
  await assert.rejects(publishUpdateFeeds(f), /Foreign asset download host/);
  assert.equal(f.writes.length, 0);
});

test('download byte limits apply while streaming, even without Content-Length', async () => {
  const asset = { name: 'test.json', size: 10 };
  await assert.rejects(downloadAsset(asset, 'v0.3.2', {
    limit: 10, fetchImpl: async () => new Response(new ReadableStream({ start(controller) { controller.enqueue(Buffer.alloc(6)); controller.enqueue(Buffer.alloc(5)); controller.close(); } })),
  }), /size limit/);
  await assert.rejects(downloadAsset(asset, 'v0.3.2', {
    limit: 10, fetchImpl: async () => new Response(Buffer.alloc(9), { headers: { 'content-length': '10' } }),
  }), /length mismatch/);
  const decoded = await downloadAsset({ name: 'test.json', size: 9 }, 'v0.3.2', {
    limit: 10, fetchImpl: async () => new Response(Buffer.alloc(9), { headers: { 'content-length': '6', 'content-encoding': 'gzip' } }),
  });
  assert.equal(decoded.length, 9, 'Fetch exposes decoded HTTP bytes');
});

test('release and build downgrades or changed identities in an existing version are rejected', async () => {
  const f = fixture();
  const old = verifyFeed(f.oldFeeds['stable.json'], HOST, { anchor: f.anchor, now: NOW });
  assert.throws(() => assertNoDowngrade(old, '0.3.0', f.selected), /downgrade/);
  const selected = structuredClone(f.selected); selected['windows-x64'].build = old.platforms['windows-x64'].build;
  assert.throws(() => assertNoDowngrade(old, '0.3.2', selected), /build downgrade/);
  assert.throws(() => assertNoDowngrade(old, '0.3.1', f.selected), /identity changed/);
  const higher = { ...f.target, id: 20, tag_name: 'v0.3.9' }; f.history.push(higher);
  await assert.rejects(publishUpdateFeeds(f), /highest stable version/);
  assert.equal(f.writes.length, 0);
  assert.equal(compareVersions('0.10.0', '0.9.99'), 1);
  assert.throws(() => compareVersions('v0.3.2', '0.3.1'), /Invalid stable version/);
});

test('a racing release asset mutation prevents any publication', async () => {
  const f = fixture({ mutateAssets: true });
  await assert.rejects(publishUpdateFeeds(f), /changed during verification/);
  assert.equal(f.writes.length, 0);
});

test('upload failure attempts to restore the previous signed feed in memory', async () => {
  const f = fixture({ sameVersion: true, age: 77, failUpload: true });
  await assert.rejects(publishUpdateFeeds(f), /GitHub API POST failed/);
  assert.deepEqual(f.blobs.get('v0.3.1/stable.json'), f.oldFeeds['stable.json']);
  assert.deepEqual(f.blobs.get('v0.3.1/legacy-stable.json'), f.oldFeeds['legacy-stable.json']);
});

test('even a valid signer cannot authorize foreign installer paths or excessive validity', () => {
  const f = fixture();
  const original = JSON.parse(Buffer.from(JSON.parse(f.oldFeeds['stable.json']).payload, 'base64'));
  function signed(payload) {
    const bytes = Buffer.from(JSON.stringify(payload));
    return Buffer.from(JSON.stringify({ payload: bytes.toString('base64'), signature: sign(null, bytes, f.privateKey).toString('base64') }));
  }
  const foreign = structuredClone(original); foreign.platforms['windows-x64'].url = 'https://attacker.example/payload.exe';
  assert.throws(() => verifyFeed(signed(foreign), HOST, { anchor: f.anchor, now: NOW }), /feed identity/);
  const long = structuredClone(original); long.expiresAt = new Date(Date.parse(long.issuedAt) + 91 * DAY).toISOString();
  assert.throws(() => verifyFeed(signed(long), HOST, { anchor: f.anchor, now: NOW }), /feed lifetime/);
});
