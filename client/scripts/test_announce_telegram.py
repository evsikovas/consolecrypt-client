import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('announce_telegram', Path(__file__).with_name('announce-telegram.py'))
announce = importlib.util.module_from_spec(spec)
spec.loader.exec_module(announce)

BODY = """# ConsoleCrypt 9.9.9

## Русский

**Новое.** Текст, перенесённый
на вторую строку, и `код` <b>не тег</b>.

- Пункт с [ссылкой](https://example.test/a)
  и продолжением.
- [Руководство](GUIDE.md)

## English

**New.** English only.
"""


def asset(name, size=50_000_000):
    return {'name': name, 'size': size, 'browser_download_url': f'https://dl.test/{name}'}


RELEASE = {
    'tag_name': 'v9.9.9',
    'name': 'ConsoleCrypt 9.9.9',
    'html_url': 'https://github.test/v9.9.9',
    'body': BODY,
    'assets': [
        asset('ConsoleCrypt-9.9.9+1-android-arm64.apk'),
        asset('ConsoleCrypt-9.9.9+1-macos-universal.dmg'),
        asset('ConsoleCrypt-9.9.9+1-windows-x64-setup.exe'),
        asset('stable.json', 1000),
        asset('SHA256SUMS-9.9.9.txt', 600),
    ],
}


class AnnounceTests(unittest.TestCase):
    def test_language_section_is_converted_to_telegram_html(self):
        text = announce.message(RELEASE, 'ru')
        self.assertIn('<b>Новое.</b> Текст, перенесённый на вторую строку, и <code>код</code> &lt;b&gt;не тег&lt;/b&gt;.', text)
        self.assertIn('• Пункт с <a href="https://example.test/a">ссылкой</a> и продолжением.\n• ', text)
        self.assertIn('docs/public/GUIDE.md">Руководство</a>', text)
        self.assertNotIn('English only', text)
        self.assertNotIn('# ConsoleCrypt', text)

    def test_download_links_are_grouped_by_platform_in_a_fixed_order(self):
        text = announce.message(RELEASE, 'en')
        self.assertIn('English only', text)
        mac, windows, android = (text.index(p) for p in ('<b>macOS</b>', '<b>Windows</b>', '<b>Android</b>'))
        self.assertLess(mac, windows)
        self.assertLess(windows, android)
        self.assertIn('<a href="https://dl.test/ConsoleCrypt-9.9.9+1-windows-x64-setup.exe">installer</a> (50 MB)', text)
        self.assertNotIn('stable.json', text)
        self.assertNotIn('SHA256SUMS', text)

    def test_long_notes_are_cut_with_a_link_and_keep_the_downloads(self):
        long = dict(RELEASE, body='## Русский\n\n' + '\n\n'.join(f'Абзац {i} ' + 'х' * 300 for i in range(40)))
        text = announce.message(long, 'ru')
        self.assertLessEqual(announce.visible_length(text), announce.LIMIT)
        self.assertIn('Полный список изменений →', text)
        self.assertIn('<b>macOS</b>', text)


if __name__ == '__main__':
    unittest.main()
