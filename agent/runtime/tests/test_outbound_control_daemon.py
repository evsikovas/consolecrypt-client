"""DRAFT metadata/budget negatives; no live HTTP/SSH/Actor/NAT acceptance."""
import io
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import threading
import time

AGENT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(AGENT),str(AGENT/'runtime')]
from cc_agent import outbound_control_daemon as daemon


class OutboundControlMetadataTests(unittest.TestCase):
    def identities(self):
        user=SimpleNamespace(pw_name='consolecrypt-agent',pw_uid=991,pw_gid=991,
            pw_dir='/var/lib/consolecrypt-agent',pw_shell='/usr/sbin/nologin')
        group=SimpleNamespace(gr_name='consolecrypt-agent',gr_gid=991)
        return user,group

    def test_ssh_subsystem_and_poller_must_share_exclusive_service_uid_not_fixture_alias(self):
        user,group=self.identities()
        daemon._same_service(user,group,[user],[group],991,991,[991])
        alias=SimpleNamespace(pw_name='ccenroll',pw_uid=991,pw_gid=991)
        for uid,gid,users,supplementary in [(10001,991,[user],[]),(991,10001,[user],[]),
            (991,991,[user,alias],[]),(991,991,[user],[991,999])]:
            with self.assertRaises(daemon.ControlUnavailable):
                daemon._same_service(user,group,users,[group],uid,gid,supplementary)

    def test_service_cannot_be_the_interactive_ssh_login_account(self):
        user,group=self.identities();user.pw_shell='/bin/sh'
        with self.assertRaises(daemon.ControlUnavailable):
            daemon._same_service(user,group,[user],[group],991,991,[991])

    def test_http_header_and_body_stream_share_one_hard_read_budget(self):
        bounded=daemon._CappedFile(io.BytesIO(b'A'*16384+b'B\n'))
        self.assertEqual(len(bounded.read(16384)),16384)
        with self.assertRaises(daemon.ControlUnavailable):bounded.readline(65537)

    def test_credential_and_json_inputs_reject_slot_purpose_duplicates_and_floats(self):
        good='ccrc_'+'A'*43
        daemon._credential(good)
        for value in [good.replace('ccrc_','ccrl_'),good[:-1]+'B',good+'\r\n']:
            with self.assertRaises(daemon.ControlUnavailable):daemon._credential(value)
        for raw in [b'{"expiresAt":1,"expiresAt":2}',b'{"limits":{"n":1.0}}',b'{"value":NaN}']:
            with self.assertRaises(daemon.ControlUnavailable):daemon._json(raw)

    def failed_bridge(self, observed=None, error=None):
        # Component result custody only: no real HTTP, SSH, Node or grant.
        stop=threading.Event();now=time.monotonic()
        data=SimpleNamespace(context=object(),end=now+60,config={
            'relayOrigin':'wss://example.invalid','relayDialIp':'127.0.0.1'},close=lambda:None)
        class Lifetime:
            def __init__(self,*_):pass
            def start(self):pass
            def check(self):pass
            def close(self):pass
        offer={'slotId':'a23e8871-910c-401c-8ab5-4f8d79e8b389','capability':'ccrl_'+'A'*43,
            'expiresInSeconds':30,'idleSeconds':10,'maxCiphertextBytes':4096}
        with patch.object(daemon._Bootstrap,'capture',return_value=data),patch.object(daemon,'_Lifetime',Lifetime),\
             patch.object(daemon,'_next',return_value=(offer,now)),\
             patch.object(daemon,'bridge_once',return_value=observed,side_effect=error):
            return daemon.run_daemon(stop)

    def test_unknown_bridge_cleanup_survives_successful_watcher_and_file_closure(self):
        observed=SimpleNamespace(end='cleanup_unknown',ssh_closed=True,carrier_closed=False)
        result=self.failed_bridge(observed)
        self.assertEqual(result['reason'],'cleanup_unknown')
        self.assertIs(result['cleanupObserved'],False)
        self.assertEqual((result['offersConsumed'],result['bridgesRetired']),(1,0))
        self.assertIs(result['resumeAuthority'],False)

    def test_bridge_exception_without_closure_observation_never_counts_retirement(self):
        result=self.failed_bridge(error=OSError('not emitted'))
        self.assertIs(result['cleanupObserved'],False)
        self.assertEqual(result['bridgesRetired'],0)
        self.assertNotIn('not emitted',repr(result))


if __name__=='__main__':unittest.main()
