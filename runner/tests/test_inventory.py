from dataclasses import replace
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.inventory import StaticInventory, parse_yaml_inventory, parse_ini_inventory, merge_inventories,apply_variable_sources


class InventoryTests(unittest.TestCase):
    def test_directory_vars_resolve_group_defaults_then_explicit_host_files_in_lexical_order(self):
        inventory=parse_ini_inventory('[web]\nservice_a ansible_host=192.0.2.10\n[web:vars]\nansible_user=base\nansible_port=22\n','inventory/hosts.ini',resolve=False)
        inventory=apply_variable_sources(inventory,[
            ('inventory/host_vars/service_a/20-last.yml','host_vars','service_a','ansible_user: final\nthreshold: 0.5\n'),
            ('inventory/group_vars/web.yml','group_vars','web','ansible_user: group\nansible_port: 2222\nenabled: true\n'),
            ('inventory/host_vars/service_a/10-first.yml','host_vars','service_a','ansible_user: first\nthreshold: 0.1\n')])
        self.assertEqual(inventory.hosts['service_a'].port,2222);self.assertEqual(inventory.hosts['service_a'].username,'final')
        self.assertEqual(inventory.hosts['service_a'].variables,{'threshold':0.5})
        self.assertEqual(inventory.to_snapshot()['groups']['web']['vars'],{'enabled':True})

    def test_directory_host_vars_can_resolve_ambiguous_sibling_connection_defaults(self):
        inventory=parse_ini_inventory('[one]\nservice_a\n[one:vars]\nansible_user=one\n[two]\nservice_a\n[two:vars]\nansible_user=two\n','inventory/hosts.ini',resolve=False)
        inventory=apply_variable_sources(inventory,[('inventory/host_vars/service_a.yml','host_vars','service_a','ansible_user: explicit\n')])
        self.assertEqual(inventory.hosts['service_a'].username,'explicit')

    def test_directory_vars_do_not_create_unknown_targets_or_introduce_credentials(self):
        for scope,target,text in [('host_vars','missing','value: 1'),('group_vars','missing','value: 1'),
            ('host_vars','service_a','ansible_password: forbidden'),('group_vars','web','ansible_connection: local'),
            ('host_vars','service_a','- not-a-mapping'),('host_vars','service_a','x: !!python/object/apply:os.system [false]')]:
            inventory=parse_ini_inventory('[web]\nservice_a\n','inventory/hosts.ini',resolve=False)
            with self.subTest(scope=scope,target=target,text=text),self.assertRaises(RunnerError):
                apply_variable_sources(inventory,[(f'inventory/{scope}/{target}.yml',scope,target,text)])
            self.assertEqual(set(inventory.hosts),{'service_a'})
    def test_static_yaml_membership_and_json_export_never_include_credentials(self):
        text = '''all:
  hosts:
    service_a:
      ansible_host: 192.0.2.10
      ansible_port: 2222
      ansible_user: deployer
  children:
    web:
      hosts:
        service_a:
    production:
      hosts:
        service_a:
'''
        inventory = parse_yaml_inventory(text, 'file-a')
        self.assertEqual(inventory.hosts['service_a'].address, '192.0.2.10')
        self.assertEqual(inventory.hosts['service_a'].port, 2222)
        self.assertEqual(inventory.groups['web']['hosts'], ['service_a'])
        self.assertEqual(inventory.groups['production']['hosts'], ['service_a'])
        exported = inventory.standard_json()
        self.assertEqual(exported['_meta']['hostvars']['service_a']['ansible_user'], 'deployer')
        inventory.hosts['service_a'] = replace(inventory.hosts['service_a'], credential_refs=('opaque-reference',))
        self.assertNotIn('opaque-reference', str(inventory.standard_json()))
        self.assertNotEqual(inventory.digest, parse_yaml_inventory(text + '# changed\n', 'file-a').digest)

    def test_static_ini_port_and_multi_group_sources_are_retained(self):
        inventory = parse_ini_inventory('''[web]
service_a ansible_host=192.0.2.10 ansible_port=2222 ansible_user=deployer
[production]
service_a
''', 'file-ini')
        self.assertEqual(inventory.hosts['service_a'].port, 2222)
        self.assertEqual(inventory.hosts['service_a'].sources, ('file-ini',))
        self.assertEqual(inventory.groups['production']['hosts'], ['service_a'])

    def test_conflicting_aliases_or_credential_bindings_never_merge_implicitly(self):
        first = parse_ini_inventory('service_a ansible_host=192.0.2.10', 'first')
        second = parse_ini_inventory('service_a ansible_host=192.0.2.11', 'second')
        with self.assertRaisesRegex(RunnerError, 'conflicts'): merge_inventories([first, second])
        second = parse_ini_inventory('service_a ansible_host=192.0.2.10', 'second')
        merged = merge_inventories([first, second])
        self.assertEqual(merged.hosts['service_a'].sources, ('first', 'second'))
        second.hosts['service_a'] = replace(second.hosts['service_a'], credential_refs=('another-credential',))
        with self.assertRaises(RunnerError): merge_inventories([first, second])

    def test_plugins_secrets_unsafe_connection_options_and_yaml_attacks_are_blocked(self):
        examples = [
            'plugin: community.aws.aws_ec2\n',
            'all: {hosts: {a: {ansible_password: actual}}}',
            'all: {hosts: {a: {ansible_ssh_common_args: "-o ProxyCommand=bad"}}}',
            'all: {hosts: {a: {ansible_connection: custom}}}',
            'all: {hosts: {a: {consolecrypt_target_id: forged}}}',
            'all: &a {hosts: {a: {}}}\nother: *a',
            'all: {hosts: {a: {}, a: {}}}',
            'all: !!python/object:builtins.object {}',
            'all: {vars: {ansible_host: "{{elsewhere}}"}, hosts: {a: {}}}',
            '_meta: {hosts: {a: {}}}',
            'web: {hosts: {web: {}}}',
            'web: {hosts: {all: {}}}',
            'all: {hosts: {a: {large_value: 9223372036854775808}}}',
        ]
        for text in examples:
            with self.subTest(example=text), self.assertRaises(RunnerError): parse_yaml_inventory(text, 'fixture')
        with self.assertRaises(RunnerError): parse_ini_inventory('a ansible_password=actual', 'fixture')
        with self.assertRaises(RunnerError): parse_ini_inventory('[a:children]\nb\n[b:children]\na', 'fixture')

    def test_group_connection_inheritance_is_resolved_before_the_visible_snapshot(self):
        inventory=parse_yaml_inventory('''all:
  vars: {ansible_user: base, ansible_port: 22}
  children:
    web:
      vars: {ansible_user: deployer, ansible_port: 2222}
      hosts:
        service_a: {ansible_host: 192.0.2.10}
        service_b: {ansible_port: 2200, ansible_user: explicit}
''','inherited')
        self.assertEqual(inventory.hosts['service_a'].username,'deployer')
        self.assertEqual(inventory.hosts['service_a'].port,2222)
        self.assertEqual(inventory.hosts['service_b'].username,'explicit')
        self.assertEqual(inventory.hosts['service_b'].port,2200)
        snapshot=inventory.to_snapshot()
        self.assertEqual(snapshot['groups']['web']['vars'],{})
        self.assertEqual(snapshot['hosts'][0]['username'],'deployer')
        self.assertEqual(StaticInventory.from_snapshot(snapshot).digest,inventory.digest)

    def test_sibling_connection_conflicts_require_an_explicit_host_value(self):
        text='''[first]
service_a
[first:vars]
ansible_user=alpha
[second]
service_a
[second:vars]
ansible_user=beta
'''
        with self.assertRaisesRegex(RunnerError,'conflict'):parse_ini_inventory(text,'siblings')
        resolved=parse_ini_inventory(text.replace('service_a\n','service_a ansible_user=explicit\n',1),'siblings')
        self.assertEqual(resolved.hosts['service_a'].username,'explicit')

    def test_merge_keeps_inherited_defaults_until_all_static_sources_are_known(self):
        defaults=parse_ini_inventory('[all:vars]\nansible_user=deployer\nansible_port=2222\n','defaults')
        hosts=parse_ini_inventory('[web]\nservice_a ansible_host=192.0.2.10\n','hosts')
        merged=merge_inventories([defaults,hosts])
        self.assertEqual(merged.hosts['service_a'].username,'deployer')
        self.assertEqual(merged.hosts['service_a'].port,2222)
        self.assertEqual(merged.standard_json()['all']['vars'],{})
        self.assertEqual(StaticInventory.from_snapshot(merged.to_snapshot()).digest,merged.digest)

    def test_shared_child_cannot_hide_an_overlong_group_path_from_validation(self):
        inventory=StaticInventory()
        for prefix in ['a','b']:
            for index in range(20):
                group=inventory.group(f'{prefix}{index}')
                if index<19:group['children']=[f'{prefix}{index+1}']
        inventory.groups['b19']['children']=['a0']
        with self.assertRaisesRegex(RunnerError,'nesting'):inventory.validate_groups()

    @unittest.skipUnless(importlib.util.find_spec('ansible') is not None,'Prepared Ansible runtime required')
    def test_directory_vars_match_installed_ansible_for_controlled_static_files(self):
        text='[web]\nservice_a ansible_host=192.0.2.10\n[web:vars]\nansible_user=base\nansible_port=22\n'
        group='ansible_user: group\nansible_port: 2222\nenabled: true\n'
        host='ansible_user: owner\nthreshold: 0.5\n'
        expected=apply_variable_sources(parse_ini_inventory(text,'hosts.ini',resolve=False),[
            ('group_vars/web.yml','group_vars','web',group),('host_vars/service_a.yml','host_vars','service_a',host)])
        with tempfile.TemporaryDirectory(prefix='cc-directory-vars-contract-') as temporary:
            root=Path(temporary);inventory=root/'inventory';inventory.mkdir()
            (inventory/'group_vars').mkdir();(inventory/'host_vars').mkdir();(inventory/'hosts.ini').write_text(text)
            (inventory/'group_vars/web.yml').write_text(group);(inventory/'host_vars/service_a.yml').write_text(host)
            config=root/'ansible.cfg';config.write_text('[inventory]\nenable_plugins=ini\n[defaults]\nvars_plugins_enabled=host_group_vars\n')
            environment={'PATH':'/usr/bin:/bin','HOME':str(root),'LANG':'C.UTF-8','ANSIBLE_CONFIG':str(config),'ANSIBLE_NOCOLOR':'1'}
            response=subprocess.run([sys.executable,'-I','-m','ansible.cli.inventory','-i',str(inventory/'hosts.ini'),'--list'],
                cwd=root,env=environment,capture_output=True,timeout=20)
            self.assertEqual(response.returncode,0,response.stderr.decode(errors='replace')[-1024:])
            import json
            actual=json.loads(response.stdout)['_meta']['hostvars']['service_a'];resolved=expected.hosts['service_a']
            self.assertEqual(actual['ansible_host'],resolved.address);self.assertEqual(actual['ansible_port'],resolved.port)
            self.assertEqual(actual['ansible_user'],resolved.username);self.assertEqual(actual['threshold'],resolved.variables['threshold'])
            self.assertEqual(actual['enabled'],True)

    @unittest.skipUnless(importlib.util.find_spec('ansible') is not None,'Prepared Ansible runtime required')
    def test_inherited_endpoints_match_the_installed_ansible_for_an_unambiguous_fixture(self):
        text='''[parent:children]
child
[parent:vars]
ansible_user=base
ansible_port=22
[child]
service_a ansible_host=192.0.2.10
service_b ansible_port=2200 ansible_user=explicit
[child:vars]
ansible_user=deployer
ansible_port=2222
'''
        expected=parse_ini_inventory(text,'upstream-fixture').standard_json()['_meta']['hostvars']
        # Only our constant inventory and an empty owned HOME/config. Never
        # invoke upstream parsing on repository-supplied plugins or scripts.
        with tempfile.TemporaryDirectory(prefix='cc-inventory-contract-') as temporary:
            root=Path(temporary);inventory=root/'inventory.ini';inventory.write_text(text)
            configuration=root/'ansible.cfg';configuration.write_text('[inventory]\nenable_plugins=ini\n')
            environment={'PATH':'/usr/bin:/bin','HOME':str(root),'LANG':'C.UTF-8',
                         'ANSIBLE_CONFIG':str(configuration),'ANSIBLE_NOCOLOR':'1'}
            response=subprocess.run([sys.executable,'-I','-m','ansible.cli.inventory','-i',str(inventory),'--list'],
                cwd=root,env=environment,capture_output=True,timeout=20)
            self.assertEqual(response.returncode,0,response.stderr.decode(errors='replace')[-1024:])
            import json
            actual=json.loads(response.stdout)['_meta']['hostvars']
            for alias,values in expected.items():
                defaults={'ansible_host':alias,'ansible_port':22,'ansible_user':None}
                for key,value in values.items():self.assertEqual(actual[alias].get(key,defaults.get(key)),value)


if __name__ == '__main__': unittest.main()
