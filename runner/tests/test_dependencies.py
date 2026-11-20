import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from consolecrypt_runner.dependencies import LOCK_FILE, check_locked_dependencies, parse_lock, subtree_digest
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.snapshot import create_snapshot


def locked_role(tree_digest):
    return {'kind':'role','name':'demo','version':'1.2.3',
            'source':{'url':'https://example.test/archive/1.2.3.tar.gz','sha256':'a'*64},
            'tree_digest':tree_digest}


class LockSchemaTests(unittest.TestCase):
    def test_future_duplicate_unpinned_and_credential_bearing_records_fail_closed(self):
        valid={'version':1,'dependencies':[locked_role('b'*64)]}
        self.assertEqual(len(parse_lock(json.dumps(valid).encode())),1)
        for mutate in (
            lambda v:v.update(version=2), lambda v:v.update(version=True),
            lambda v:v.update(unknown='not-ignored'),
            lambda v:v['dependencies'].append(copy.deepcopy(v['dependencies'][0])),
            lambda v:v['dependencies'][0].update(name='../outside'),
            lambda v:v['dependencies'][0].update(version='latest'),
            lambda v:v['dependencies'][0].update(tree_digest='unfixed'),
            lambda v:v['dependencies'][0]['source'].update(url='http://example.test/role'),
            lambda v:v['dependencies'][0]['source'].update(url='https://user:secret@example.test/role'),
            lambda v:v['dependencies'][0]['source'].update(url='https://example.test/role?token=private'),
            lambda v:v['dependencies'][0]['source'].update(sha256=None),
            lambda v:v['dependencies'][0].update(kind='collection',name='ansible.builtin'),
        ):
            value=copy.deepcopy(valid);mutate(value)
            with self.subTest(value=value),self.assertRaises(RunnerError):parse_lock(json.dumps(value).encode())
        with self.assertRaises(RunnerError):parse_lock(b'{"version":1,"version":1,"dependencies":[]}')
        with self.assertRaises(RunnerError):parse_lock(json.dumps(dict(valid,dependencies=valid['dependencies']*33)).encode())
        with self.assertRaises(RunnerError):parse_lock(b' '*(64*1024+1))


@unittest.skipUnless(os.name=='posix','Unix snapshot helper')
class DependencyTreeTests(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='cc-locked-dependency-')
        self.addCleanup(temporary.cleanup)
        self.root=Path(temporary.name);self.project=self.root/'project';self.project.mkdir()
        role=self.project/'roles/demo';(role/'tasks').mkdir(parents=True)
        (role/'tasks/main.yml').write_text('---\n- name: Fixture\n  ansible.builtin.debug:\n    msg: fixture\n')
        self.role=role

    def snapshot(self,name):
        return create_snapshot(self.project,self.root/name).to_json()

    def lock(self,digest):
        (self.project/LOCK_FILE).write_text(json.dumps({'version':1,'dependencies':[locked_role(digest)]}))

    def test_missing_and_empty_lock_do_not_install_or_contact_any_source(self):
        with patch('socket.create_connection',side_effect=AssertionError('No network')) as network:
            self.assertEqual(check_locked_dependencies(self.project),0)
            (self.project/LOCK_FILE).write_text('{"version":1,"dependencies":[]}')
            self.assertEqual(check_locked_dependencies(self.project),0)
            network.assert_not_called()

    def test_projection_matches_an_independent_tree_and_validates_roles_and_collections(self):
        collection=self.project/'collections/ansible_collections/example/demo';collection.mkdir(parents=True)
        (collection/'galaxy.yml').write_text('namespace: example\nname: demo\nversion: 1.2.3\n')
        (self.role/'empty').mkdir()
        snapshot=self.snapshot('first')
        digest,_=subtree_digest(snapshot,'roles/demo')
        standalone=create_snapshot(self.role,self.root/'standalone')
        self.assertEqual(digest,standalone.digest)
        other,_=subtree_digest(snapshot,'collections/ansible_collections/example/demo')
        contract={'version':1,'dependencies':[locked_role(digest),dict(locked_role(other),kind='collection',name='example.demo')]}
        (self.project/LOCK_FILE).write_text(json.dumps(contract))
        self.assertEqual(check_locked_dependencies(self.project,self.snapshot('locked')),2)

    def test_changed_tree_missing_dependency_and_corrupt_snapshot_are_not_success(self):
        before=self.snapshot('first');digest,_=subtree_digest(before,'roles/demo');self.lock(digest)
        self.assertEqual(check_locked_dependencies(self.project,self.snapshot('valid')),1)
        (self.role/'tasks/main.yml').write_text('changed code')
        with self.assertRaisesRegex(RunnerError,'pinned tree digest'):
            check_locked_dependencies(self.project,self.snapshot('changed'))
        broken=copy.deepcopy(before);broken['entries'].clear()
        with self.assertRaises(RunnerError):check_locked_dependencies(self.project,broken)
        with self.assertRaises(RunnerError):subtree_digest(before,'roles/absent')

    def test_absent_entry_point_or_non_regular_lock_is_rejected_without_following_it(self):
        (self.role/'tasks/main.yml').unlink()
        snapshot=self.snapshot('empty');digest,_=subtree_digest(snapshot,'roles/demo');self.lock(digest)
        with self.assertRaisesRegex(RunnerError,'entry point'):check_locked_dependencies(self.project,self.snapshot('locked'))
        path=self.project/LOCK_FILE;path.unlink();path.mkdir()
        with self.assertRaises((RunnerError,OSError)):check_locked_dependencies(self.project)
        path.rmdir();outside=self.root/'outside';outside.write_text('synthetic-private-content')
        path.symlink_to(outside)
        with self.assertRaises((RunnerError,OSError)):check_locked_dependencies(self.project)

    def test_wrong_case_lock_never_degrades_to_an_absent_optional_file(self):
        (self.project/LOCK_FILE.upper()).write_text('{"version":1,"dependencies":[]}')
        with self.assertRaisesRegex(RunnerError,'exact lowercase'):
            check_locked_dependencies(self.project)
