import importlib.util
import io
import json
import os
from pathlib import Path
import unittest
from unittest.mock import patch
import urllib.error

spec = importlib.util.spec_from_file_location('announce_evsikovnet', Path(__file__).with_name('announce-evsikovnet.py'))
announce = importlib.util.module_from_spec(spec)
spec.loader.exec_module(announce)

RELEASE = {
    'tag_name': 'v9.9.9',
    'name': 'ConsoleCrypt 9.9.9',
    'draft': False,
    'prerelease': False,
    'published_at': '2026-10-27T21:30:00Z',
    'body': '# ConsoleCrypt 9.9.9\n\n## Русский\n\n- Новое, см. [руководство](GUIDE.md) и [сайт](https://consolecrypt.dev).\n\n## English\n\n- English only.\n',
}


class _Response(io.BytesIO):
    def __init__(self, status, value):
        super().__init__(json.dumps(value).encode())
        self.status = status


class _Opener:
    """Answers evsikov.net API calls from a script of (method, path) → reply."""

    def __init__(self, replies):
        self.replies = replies
        self.calls = []

    def open(self, request, timeout):
        path = request.full_url[len(announce.API):]
        self.calls.append((request.get_method(), path, request.get_header('Authorization'), request.data))
        status, value = self.replies[(request.get_method(), path)]
        if status >= 400:
            raise urllib.error.HTTPError(request.full_url, status, 'error', {}, io.BytesIO(json.dumps(value).encode()))
        return _Response(status, value)


class EvsikovNetTests(unittest.TestCase):
    def test_body_has_the_russian_notes_and_dates_without_downloads(self):
        body = announce.payload(RELEASE)
        self.assertEqual(body['project_id'], announce.PROJECT_ID)
        self.assertEqual((body['version'], body['title'], body['released_at']), ('9.9.9', 'ConsoleCrypt 9.9.9', '2026-10-27'))
        self.assertIn('[руководство](https://github.com/evsikovas/consolecrypt-client/blob/main/docs/public/GUIDE.md)', body['notes_md'])
        self.assertIn('[сайт](https://consolecrypt.dev)', body['notes_md'])
        self.assertNotIn('English only', body['notes_md'])
        self.assertEqual(body['downloads'], [])

    def test_drafts_prereleases_and_other_tags_are_refused(self):
        for change in ({'draft': True}, {'prerelease': True}, {'tag_name': 'v9.9.9-rc1'}):
            with self.assertRaises(SystemExit):
                announce.payload(dict(RELEASE, **change))

    def test_new_version_is_created_with_the_token(self):
        opener = _Opener({('POST', '/releases'): (201, {'id': 'r1'})})
        self.assertEqual(announce.publish(announce.payload(RELEASE), 'secret', opener), 'created 9.9.9 (r1)')
        self.assertEqual(opener.calls[0][2], 'Bearer secret')

    def test_an_existing_version_is_updated_in_place(self):
        opener = _Opener({
            ('POST', '/releases'): (409, {'error': 'exists'}),
            ('GET', '/public/releases?project=consolecrypt'): (200, [{'id': 'old', 'version': '9.9.8'}, {'id': 'r9', 'version': '9.9.9'}]),
            ('PUT', '/releases/r9'): (200, {'id': 'r9'}),
        })
        self.assertEqual(announce.publish(announce.payload(RELEASE), 'secret', opener), 'updated 9.9.9 (r9)')
        public = next(call for call in opener.calls if call[0] == 'GET')
        self.assertIsNone(public[2], 'the public list never receives the token')

    def test_refusals_stop_with_the_reason(self):
        with self.assertRaises(SystemExit):
            announce.publish(announce.payload(RELEASE), 'secret', _Opener({('POST', '/releases'): (400, {'error': 'Validation failed'})}))
        hidden = _Opener({
            ('POST', '/releases'): (409, {}),
            ('GET', '/public/releases?project=consolecrypt'): (200, []),
        })
        with self.assertRaises(SystemExit):
            announce.publish(announce.payload(RELEASE), 'secret', hidden)

    def test_publishing_outside_the_release_ci_is_refused(self):
        with patch.object(announce, 'github_release', return_value=RELEASE), \
                patch.object(announce, 'publish') as publish, \
                patch.dict(os.environ, {'EVSIKOVNET_TOKEN': 'secret', 'GITHUB_ACTIONS': '', 'GITHUB_REF': ''}), \
                patch('sys.argv', ['announce-evsikovnet.py', '--tag', 'v9.9.9']):
            with self.assertRaises(SystemExit):
                announce.main()
        publish.assert_not_called()


if __name__ == '__main__':
    unittest.main()
