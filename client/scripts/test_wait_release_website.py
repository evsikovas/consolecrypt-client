import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('website', Path(__file__).with_name('wait-release-website.py'))
website = importlib.util.module_from_spec(spec)
spec.loader.exec_module(website)


class DownloadReadiness(unittest.TestCase):
    def setUp(self):
        suffixes = ['macos-universal.dmg', 'windows-x64-setup.exe', 'windows-x64.zip',
                    'linux-x64.deb', 'linux-x64.rpm', 'android-arm64.apk', 'ios-simulator-universal.zip']
        self.urls = [f'https://github.com/evsikovas/consolecrypt-client/releases/download/v1.0.0/ConsoleCrypt-1.0.0%2B61015-{suffix}' for suffix in suffixes]
        self.links = ''.join(f'<a href="{url}">Download</a>' for url in self.urls)
        self.heading = '<p>Текущий релиз · 1.0.0</p>'

    def test_complete_current_page_is_ready_with_encoded_or_literal_plus(self):
        self.assertTrue(website.ready(self.heading + self.links, '1.0.0', self.urls))
        self.assertTrue(website.ready(self.heading + self.links.replace('%2B', '+'), '1.0.0', self.urls))

    def test_missing_simulator_wrong_build_or_previous_heading_holds_announcements(self):
        for content in (self.heading + self.links[:self.links.rindex('<a ')],
                        self.heading + self.links.replace('61015-ios', '61025-ios'),
                        self.heading.replace('1.0.0', '0.9.15') + self.links):
            with self.subTest(content=content):
                self.assertFalse(website.ready(content, '1.0.0', self.urls))

    def test_foreign_or_commented_links_are_not_downloads(self):
        for content in (self.heading + '<!--' + self.links + '-->',
                        self.heading + self.links.replace('github.com', 'github.com.example.test'),
                        self.heading + self.links.replace('https://', 'https%3A%2F%2F')):
            self.assertFalse(website.ready(content, '1.0.0', self.urls))
        self.assertFalse(website.ready(self.heading + self.links, '1.0.0', []))


if __name__ == '__main__':
    unittest.main()
