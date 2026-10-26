import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).parent


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), HERE / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


verify = load('verify-client-release')
update = load('update-release-docs')
REPO = verify.REPOSITORY
SUFFIXES = list(verify.PACKAGES)
BUILDS = {'macos': 46011, 'windows': 46012, 'linux': 46013, 'android': 46014}


def release(version='0.9.3'):
    tag = f'v{version}'
    sums, assets = [], []

    def add(name, size, digest=None):
        assets.append({
            'name': name, 'size': size, 'state': 'uploaded', 'digest': digest and f'sha256:{digest}',
            'browser_download_url': f'https://github.com/{REPO}/releases/download/{tag}/{name.replace("+", "%2B")}',
        })

    for index, suffix in enumerate(SUFFIXES):
        name = f'ConsoleCrypt-{version}+{BUILDS[verify.PACKAGES[suffix][0]]}-{suffix}'
        digest = f'{index:x}' * 64
        sums.append(f'{digest}  {name}')
        add(name, 1_000_000 * (index + 1), digest)
    for feed in ('stable.json', 'legacy-stable.json', 'linux-stable.json'):
        add(feed, 900)
    add(f'SHA256SUMS-{version}.txt', 700)
    data = {'tag_name': tag, 'draft': False, 'prerelease': False, 'published_at': '2026-10-28T10:00:00Z',
            'html_url': f'https://github.com/{REPO}/releases/tag/{tag}', 'assets': assets}
    return data, '\n'.join(sums) + '\n'


class VerifyReleaseTests(unittest.TestCase):
    def test_a_complete_release_lists_every_installer_with_its_checksum(self):
        verified = verify.validate(*release())
        self.assertEqual(verified['builds'], BUILDS)
        self.assertEqual([item['suffix'] for item in verified['installers']], SUFFIXES)
        self.assertEqual(verified['feeds'], ['legacy-stable.json', 'linux-stable.json', 'stable.json'])
        dmg = verified['installers'][0]
        self.assertEqual(dmg['url'], f'https://github.com/{REPO}/releases/download/v0.9.3/ConsoleCrypt-0.9.3%2B46011-macos-universal.dmg')
        self.assertFalse(verified['binaries_verified'])

    def test_broken_releases_are_refused(self):
        def broken(change):
            data, sums = release()
            data = copy.deepcopy(data)
            result = change(data, sums)
            sums = result if isinstance(result, str) else sums
            with self.assertRaises(verify.ReleaseError):
                verify.validate(data, sums)

        broken(lambda d, s: d.update(draft=True))
        broken(lambda d, s: d.update(tag_name='v0.9.3-rc1'))
        broken(lambda d, s: d['assets'].pop(0))  # a missing installer
        broken(lambda d, s: d['assets'][0].update(state='starter'))
        broken(lambda d, s: d['assets'][0].update(digest='sha256:' + 'f' * 64))
        broken(lambda d, s: d['assets'][0].update(size=verify.LIMIT + 1))
        broken(lambda d, s: d['assets'][1].update(browser_download_url='https://example.test/file'))
        broken(lambda d, s: d['assets'].append(dict(d['assets'][0], name='notes.txt')))
        # Two Windows packages with different builds.
        def split_build(d, s):
            zip_asset = d['assets'][2]
            new = zip_asset['name'].replace('+46012-', '+46019-')
            zip_asset['browser_download_url'] = zip_asset['browser_download_url'].replace('%2B46012-', '%2B46019-')
            zip_asset['name'] = new
            return s.replace('ConsoleCrypt-0.9.3+46012-windows-x64.zip', new)
        broken(split_build)
        broken(lambda d, s: s + s.splitlines()[0] + '\n')  # duplicate checksum


README = """# ConsoleCrypt — документация

Руководство
описывает клиент 0.9.2; таблица готовых сборок ниже относится к последнему
опубликованному выпуску.

- [Обновления](docs/public/UPDATES.md), [версия 0.9.2](docs/public/RELEASE_0_9_2.md), [версия 0.9.1](docs/public/RELEASE_0_9_1.md).

## Готовые сборки 0.9.2

Шесть файлов выпущены из одного тега [v0.9.2](https://github.com/{repo}/releases/tag/v0.9.2).
Перед установкой сверьте файл с [SHA-256 релиза](https://github.com/{repo}/releases/download/v0.9.2/SHA256SUMS-0.9.2.txt).

| Платформа | Сборка | Файл | Размер, байт |
| --- | --- | --- | ---: |
| macOS, Apple Silicon и Intel | 0.9.2+45011 | [DMG](https://github.com/{repo}/releases/download/v0.9.2/ConsoleCrypt-0.9.2%2B45011-macos-universal.dmg) | 192 583 745 |
| Android ARM64 | 0.9.2+45014 | [APK](https://github.com/{repo}/releases/download/v0.9.2/ConsoleCrypt-0.9.2%2B45014-android-arm64.apk) | 123 786 744 |
| iOS Simulator на Mac (прежняя сборка) | 0.5.6+23015 | [ZIP](https://github.com/{repo}/releases/download/v0.5.6/ConsoleCrypt-0.5.6%2B23015-ios-simulator-universal.zip) | 154 939 478 |

Выпуск 0.9.2 для iOS отложен; доступна прежняя предварительная сборка 0.5.6.
""".replace('{repo}', REPO)

OLD_BUILDS = {'macos-universal.dmg': 45011, 'windows-x64-setup.exe': 45012, 'windows-x64.zip': 45012,
              'linux-x64.deb': 45013, 'linux-x64.rpm': 45013, 'android-arm64.apk': 45014}
DOWNLOAD = (
    '<p class="download-version" data-i18n>Текущий релиз · 0.9.2</p>'
    f'<a href="https://github.com/{REPO}/releases/tag/v0.9.2">GitHub</a>'
    '<p data-i18n>Универсальный DMG · 0.9.2+45011</p><span data-i18n>Пакеты · 0.9.2+45013</span>'
    + ''.join(
        f'<a href="https://github.com/{REPO}/releases/download/v0.9.2/ConsoleCrypt-0.9.2%2B{build}-{suffix}" '
        f'data-umami-event-version="0.9.2+{build}">↓</a>'
        for suffix, build in OLD_BUILDS.items()
    )
)
INDEX = '<p class="hero-note" data-i18n>macOS <span data-i18n>v0.9.2 · ранняя версия</span></p>'
EN = (
    '  "Пакеты · 0.9.2+45013": "Packages · 0.9.2+45013",\n'
    "  'v0.9.2 · ранняя версия': 'v0.9.2 · early release',\n"
    "  'Универсальный DMG · 0.9.2+45011': 'Universal DMG · 0.9.2+45011',\n"
    '  "Текущий релиз · 0.9.2": "Current release · 0.9.2",\n'
)


class UpdateDocsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        self.docs, self.site = root / 'docs', root / 'site'
        (self.site / 'assets').mkdir(parents=True)
        self.docs.mkdir()
        (self.docs / 'README.md').write_text(README)
        (self.site / 'download.html').write_text(DOWNLOAD)
        (self.site / 'index.html').write_text(INDEX)
        (self.site / 'assets/en.js').write_text(EN)
        self.verified = root / 'verified.json'
        self.verified.write_text(json.dumps(verify.validate(*release())))

    def tearDown(self):
        self.temp.cleanup()

    def run_script(self, *extra):
        return subprocess.run(
            [sys.executable, str(HERE / 'update-release-docs.py'), str(self.verified), '--docs', str(self.docs), '--site', str(self.site), *extra],
            capture_output=True, text=True,
        )

    def test_docs_and_site_point_at_the_new_release(self):
        self.assertEqual(self.run_script('--check').returncode, 1)
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        readme = (self.docs / 'README.md').read_text()
        self.assertIn('описывает клиент 0.9.3;', readme)
        self.assertIn('[версия 0.9.3](docs/public/RELEASE_0_9_3.md), [версия 0.9.2](docs/public/RELEASE_0_9_2.md)', readme)
        self.assertIn('## Готовые сборки 0.9.3', readme)
        self.assertIn('| Windows x64 | 0.9.3+46012 | [Portable ZIP](', readme)
        self.assertIn('| Linux x64 | 0.9.3+46013 | [RPM](', readme)
        self.assertIn(' | 5 000 000 |', readme)
        self.assertIn('| iOS Simulator на Mac (прежняя сборка) | 0.5.6+23015 |', readme)
        self.assertIn('Выпуск 0.9.3 для iOS отложен', readme)
        download = (self.site / 'download.html').read_text()
        self.assertIn('Текущий релиз · 0.9.3', download)
        self.assertIn(f'{REPO}/releases/tag/v0.9.3', download)
        self.assertIn('v0.9.3/ConsoleCrypt-0.9.3%2B46013-linux-x64.deb', download)
        self.assertIn('data-umami-event-version="0.9.3+46012"', download)
        self.assertIn('v0.9.3 · ранняя версия', (self.site / 'index.html').read_text())
        en = (self.site / 'assets/en.js').read_text()
        self.assertIn('"Пакеты · 0.9.3+46013": "Packages · 0.9.3+46013"', en)
        self.assertIn('"Current release · 0.9.3"', en)
        for path in [self.site / 'download.html', self.site / 'index.html', self.site / 'assets/en.js']:
            self.assertNotIn('0.9.2', path.read_text(), path)
        # A second run has nothing left to do.
        self.assertEqual(self.run_script('--check').returncode, 0)

    def test_an_unknown_mention_of_the_old_version_writes_nothing(self):
        (self.site / 'index.html').write_text(INDEX + '<p>Since 0.9.2 we ship faster</p>')
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Still mentions 0.9.2', result.stderr)
        self.assertIn('Текущий релиз · 0.9.2', (self.site / 'download.html').read_text())
        self.assertIn('## Готовые сборки 0.9.2', (self.docs / 'README.md').read_text())


if __name__ == '__main__':
    unittest.main()
