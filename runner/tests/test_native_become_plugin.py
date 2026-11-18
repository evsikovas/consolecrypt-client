"""Real Ansible plugin composition; fake RPC, never connects to any SSH host."""
import base64
import os
import unittest
from unittest.mock import patch
from ansible.errors import AnsibleConnectionFailure
from ansible.playbook.play_context import PlayContext
from ansible.plugins.loader import become_loader, shell_loader
from ansible.plugins.action import ActionBase
from ansible.parsing.dataloader import DataLoader
from ansible.playbook.task import Task
from ansible.template import Templar
from consolecrypt_runner.plugins.connection.consolecrypt_rpc import Connection

class SyntheticAction(ActionBase):
    def run(self,tmp=None,task_vars=None):
        raise AssertionError('test only invokes the real low-level ActionBase composition')

class NativeBecomePluginTest(unittest.TestCase):
    def connection(self):
        context=PlayContext();context.remote_addr='synthetic.invalid';context.remote_user='fixture'
        connection=Connection(context,None)
        calls=[]
        def rpc(operation,body):
            calls.append((operation,body))
            return {'exit_code':0,'stdout':base64.b64encode(b'fixture-data').decode(),'stderr':''}
        connection._rpc=rpc
        return connection,calls
    def sudo(self):
        plugin=become_loader.get('sudo')
        plugin.set_options(direct={'become_user':'root','become_flags':'-H -S -n','become_exe':'sudo'})
        return plugin
    @patch.dict(os.environ,{'CC_NATIVE_SUDO':'1'})
    def test_unwrapped_command_and_nonsecret_descriptor_only(self):
        connection,calls=self.connection();plugin=self.sudo();connection.set_become_plugin(plugin)
        raw="printf 'ordinary module input'"
        wrapped=plugin.build_become_command(raw,shell_loader.get('sh'))
        self.assertNotEqual(wrapped,raw)
        self.assertEqual(connection.exec_command(wrapped,b'binary\x00data')[0],0)
        operation,body=calls.pop()
        self.assertEqual(operation,'exec_become')
        self.assertEqual(body,{'method':'sudo','user':'root','command':raw,'stdin':base64.b64encode(b'binary\x00data').decode()})
        self.assertNotIn('password',body)
    @patch.dict(os.environ,{'CC_NATIVE_SUDO':'1'})
    def test_actual_actionbase_composes_native_sudo_without_outer_shell_rewrite(self):
        connection,calls=self.connection();plugin=self.sudo();connection.set_become_plugin(plugin)
        loader=DataLoader();context=connection._play_context
        action=SyntheticAction(Task(),connection,context,loader,Templar(loader=loader),None)
        result=action._low_level_execute_command('id',sudoable=True)
        self.assertEqual(result['rc'],0)
        self.assertEqual(calls[0][0],'exec_become');self.assertEqual(calls[0][1]['command'],'id')
    @patch.dict(os.environ,{'CC_NATIVE_SUDO':'0'})
    def test_existing_passwordless_path_is_unchanged(self):
        connection,calls=self.connection();plugin=self.sudo();connection.set_become_plugin(plugin)
        wrapped=plugin.build_become_command('id',shell_loader.get('sh'))
        connection.exec_command(wrapped)
        self.assertEqual(calls[0][0],'exec');self.assertEqual(calls[0][1]['command'],wrapped)
    @patch.dict(os.environ,{'CC_NATIVE_SUDO':'1'})
    def test_repeated_plugin_selection_does_not_nest_wrappers(self):
        connection,calls=self.connection();plugin=self.sudo()
        for _ in range(1200): connection.set_become_plugin(plugin)
        wrapped=plugin.build_become_command('id',shell_loader.get('sh'))
        connection.exec_command(wrapped)
        self.assertEqual(calls[0][0],'exec_become')
    @patch.dict(os.environ,{'CC_NATIVE_SUDO':'1'})
    def test_unsupported_method_and_executable_fail_before_rpc(self):
        connection,calls=self.connection()
        with self.assertRaises(AnsibleConnectionFailure): connection.set_become_plugin(become_loader.get('su'))
        plugin=self.sudo();plugin.set_options(direct={'become_exe':'/tmp/caller-controlled'})
        with self.assertRaises(AnsibleConnectionFailure): connection.set_become_plugin(plugin)
        self.assertEqual(calls,[])

if __name__=='__main__': unittest.main()
