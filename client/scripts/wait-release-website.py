#!/usr/bin/env python3
"""Wait for the public download page to link every verified release package."""
import argparse
from html.parser import HTMLParser
import importlib.util
import json
from pathlib import Path
import re
import sys
import time
from urllib.parse import unquote, urlsplit
from urllib.request import Request, urlopen

WEBSITE = 'https://consolecrypt.dev/download'


def identity(url):
    parsed = urlsplit(url)
    if (parsed.scheme != 'https' or parsed.hostname != 'github.com'
            or parsed.username or parsed.password or parsed.port not in (None, 443)
            or parsed.query or parsed.fragment):
        return None
    return unquote(parsed.path)


class DownloadPage(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links, self.text = set(), []

    def handle_starttag(self, tag, attrs):
        if tag == 'a':
            href = dict(attrs).get('href', '')
            try:
                if value := identity(href):
                    self.links.add(value)
            except ValueError:
                pass

    def handle_data(self, data):
        self.text.append(data)


def ready(contents, version, urls):
    page = DownloadPage()
    page.feed(contents)
    expected = {identity(url) for url in urls}
    return (bool(expected) and None not in expected and expected <= page.links
            and f'Текущий релиз · {version}' in ' '.join(' '.join(page.text).split()))


def release_links(tag):
    spec = importlib.util.spec_from_file_location('verify_release', Path(__file__).with_name('verify-client-release.py'))
    verifier = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(verifier)
    with verifier.fetch(f'https://api.github.com/repos/{verifier.REPOSITORY}/releases/tags/{tag}', 'application/vnd.github+json') as response:
        release = json.load(response)
    asset = next(a for a in release['assets'] if a['name'] == f'SHA256SUMS-{tag[1:]}.txt')
    with verifier.fetch(asset['browser_download_url']) as response:
        sums = response.read(65537).decode('utf-8')
    verified = verifier.validate(release, sums)
    return [item['url'] for item in verified['installers']]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--timeout-seconds', type=int, default=1800)
    args = parser.parse_args()
    if not re.fullmatch(r'v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', args.tag) or not 1 <= args.timeout_seconds <= 1800:
        parser.error('Require a stable tag and timeout of 1..1800 seconds')
    links = release_links(args.tag)
    deadline = time.monotonic() + args.timeout_seconds
    while True:
        try:
            request = Request(WEBSITE, headers={'User-Agent': 'ConsoleCrypt-release-check', 'Cache-Control': 'no-cache'})
            with urlopen(request, timeout=20) as response:
                contents = response.read(2 * 1024 * 1024 + 1)
            if len(contents) <= 2 * 1024 * 1024 and ready(contents.decode('utf-8'), args.tag[1:], links):
                print(f'Website ready: {args.tag}, {len(links)} verified package links')
                return 0
        except (OSError, UnicodeError, ValueError):
            pass
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            print(f'Website is not ready for {args.tag}; announcements withheld', file=sys.stderr)
            return 1
        print(f'Waiting for website downloads: {args.tag}', flush=True)
        time.sleep(min(20, remaining))


if __name__ == '__main__':
    sys.exit(main())
