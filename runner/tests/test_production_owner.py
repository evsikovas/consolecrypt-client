import base64
import copy
import hashlib
import time
import unittest
import uuid

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from consolecrypt_runner.approval import DOMAIN, TrustGrant, verify_envelope
from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.production_owner import OWNER_DOMAIN, OWNER_POLICY, ManualInteractiveGrant
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.service.service_helper import ServiceHelper


class ProductionOwnerTests(unittest.TestCase):
    def setUp(self):
        self.key = Ed25519PrivateKey.generate()
        self.signer, self.tenant, self.runner, self.project, self.host = [str(uuid.uuid4()) for _ in range(5)]
        self.now = int(time.time())
        self.manifest = {
            'version': 1, 'job_id': str(uuid.uuid4()), 'tenant_id': self.tenant, 'runner_id': self.runner,
            'project_id': self.project, 'revision_digest': 'a'*64, 'inventory_digest': 'b'*64,
            'environment_digest': 'c'*64, 'parameters_digest': 'd'*64, 'playbook': 'site.yml', 'mode': 'apply',
            'policy': {'strict_isolation': True, 'cancel_on_lock': True, 'timeout_seconds': 300, 'forks': 1, 'production': True},
            'targets': [{'host_id': self.host, 'alias': 'owned', 'address': '127.0.0.1', 'port': 22,
                         'username': 'runner', 'route_digest': 'e'*64, 'credential_refs': []}],
            'issued_at': self.now, 'expires_at': self.now+300, 'access_expires_at': self.now+300, 'nonce': str(uuid.uuid4())}
        self.config = {'tenant_id': self.tenant, 'runner_id': self.runner, 'signer_id': self.signer,
                       'public_key': base64.b64encode(self.key.public_key().public_bytes_raw()).decode(),
                       'hosts': [self.host], 'projects': [self.project], 'expires_at': self.now+3500,
                       'max_job_seconds': 3600, 'transport': 'native_scoped_rpc_v1'}
        self.executor = InteractiveExecutor(None, None, None)
        self.addCleanup(self.executor.worker.shutdown, wait=True)
        self.executor.configure(self.config)

    def signed(self, manifest=None, *, owner=True):
        m = copy.deepcopy(self.manifest if manifest is None else manifest)
        raw = DOMAIN + encode(m)
        result = {'signer_id': self.signer, 'manifest': m, 'signature': base64.b64encode(self.key.sign(raw)).decode()}
        if owner:
            digest = hashlib.sha256(raw).digest()
            body = OWNER_DOMAIN + b'\x01' + uuid.UUID(self.signer).bytes + digest
            result['owner_confirmation'] = {'version': 1, 'policy': OWNER_POLICY, 'signer_id': self.signer,
                                            'manifest_digest': digest.hex(), 'signature': base64.b64encode(self.key.sign(body)).decode()}
        return result

    def verify(self, envelope):
        return verify_envelope(envelope, self.executor.grants, tenant_id=self.tenant, runner_id=self.runner,
                               environment_digest='c'*64, now=self.now)

    def manual(self):
        result = self.executor.configure_manual_owner(copy.deepcopy(self.config))
        self.assertEqual(result, {'configured': True, 'persistent': False, 'owner_policy': OWNER_POLICY})

    def test_same_enrolled_manual_owner_can_verify_exact_production_only_with_extra_signature(self):
        self.manual()
        self.assertEqual(self.verify(self.signed()).value, self.manifest)
        with self.assertRaises(RunnerError): self.verify(self.signed(owner=False))

    def test_service_internal_neutral_configure_and_valid_node_signatures_still_deny(self):
        # This is the actual configure() used internally by Service, not a
        # caller-supplied production/source flag or a mocked grant decoder.
        self.assertIs(type(self.executor.grants[self.signer]), TrustGrant)
        with self.assertRaisesRegex(RunnerError, 'original manual owner scope'):
            self.verify(self.signed())
        with self.assertRaises(TypeError): ManualInteractiveGrant(None, self.executor.grants[self.signer])

    def test_service_endpoint_cannot_call_manual_configuration(self):
        service = object.__new__(ServiceHelper)
        service.runtime = object()
        with self.assertRaises(RunnerError):
            service.request('configure_manual_owner', self.config, None)
        self.assertIs(type(self.executor.grants[self.signer]), TrustGrant)

    def test_manual_operation_cannot_enroll_replace_key_renew_or_change_scope(self):
        for field, value in [('public_key', base64.b64encode(Ed25519PrivateKey.generate().public_key().public_bytes_raw()).decode()),
                             ('expires_at', self.config['expires_at']+1), ('hosts', []), ('projects', []),
                             ('tenant_id', str(uuid.uuid4())), ('runner_id', str(uuid.uuid4()))]:
            changed = copy.deepcopy(self.config); changed[field] = value
            with self.assertRaises(RunnerError): self.executor.configure_manual_owner(changed)
            self.assertIs(type(self.executor.grants[self.signer]), TrustGrant)
        self.executor.vault_is_locked = True
        with self.assertRaises(RunnerError): self.manual()

    def test_agreement_cannot_follow_reminted_job_nonce_deadline_or_changed_parameters_privileges(self):
        self.manual()
        old = self.signed()['owner_confirmation']
        for field, value in [('job_id', str(uuid.uuid4())), ('nonce', str(uuid.uuid4())),
                             ('expires_at', self.now+299), ('parameters_digest', 'f'*64)]:
            changed = copy.deepcopy(self.manifest); changed[field] = value
            envelope = self.signed(changed); envelope['owner_confirmation'] = old
            with self.assertRaises(RunnerError): self.verify(envelope)
        changed = copy.deepcopy(self.manifest); changed['targets'][0]['username'] = 'root'
        envelope = self.signed(changed); envelope['owner_confirmation'] = old
        with self.assertRaises(RunnerError): self.verify(envelope)

    def test_job_signature_future_policy_wrong_signer_extra_fields_never_become_agreement(self):
        self.manual()
        for field, value in [('signature', self.signed()['signature']), ('policy', 'futurePolicy'),
                             ('version', True), ('signer_id', str(uuid.uuid4())), ('approved', True)]:
            envelope = self.signed(); envelope['owner_confirmation'][field] = value
            with self.assertRaises(RunnerError): self.verify(envelope)

    def test_nonproduction_wire_and_api_unchanged_extra_evidence_rejected(self):
        m = copy.deepcopy(self.manifest); m['policy']['production'] = False
        envelope = self.signed(m, owner=False)
        self.assertEqual(set(envelope), {'signer_id', 'manifest', 'signature'})
        self.assertFalse(self.verify(envelope).value['policy']['production'])
        self.manual()
        self.assertFalse(self.verify(envelope).value['policy']['production'])
        with self.assertRaises(RunnerError): self.verify(self.signed(m))


if __name__ == '__main__': unittest.main()
