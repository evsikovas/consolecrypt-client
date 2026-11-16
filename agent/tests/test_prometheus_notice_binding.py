"""Synthetic metadata causals only; no normal ELF/build/runtime/signing claim."""
from pathlib import Path
from contextlib import contextmanager
from unittest.mock import patch
import base64,copy,hashlib,importlib.util,io,json,subprocess,sys,tempfile,unittest,zipfile
DELIVERY=Path(__file__).resolve().parents[1]/'delivery'

def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec)
    sys.modules[name]=module;spec.loader.exec_module(module);return module
def encoded(value):return(json.dumps(value,sort_keys=True)+'\n').encode()
def digest(raw):return hashlib.sha256(raw).hexdigest()

class CompactInventoryEncoding(unittest.TestCase):
    def test_full_notice_inventory_is_compact_without_dropping_fields_or_texts(self):
        with patch.object(sys,'path',[str(DELIVERY),*sys.path]):
            generator=load('compact_inventory_producer',DELIVERY/'notice_generate.py')
            consumer=load('compact_inventory_consumer',DELIVERY/'notice_inventory.py')
        full=b'Synthetic complete notice text.\n';h=digest(full);path='notices/text/'+h+'.txt'
        component={'notices':[{'original_path':'LICENSE','package_path':path,'sha256':h}],
            'source_file_sha256':{'src/module'+str(i)+'.rs':'a'*64 for i in range(8)}}
        value={'format':'cc-agent-notice-inventory/v1',
            'components':[dict(component,name='synthetic'+str(i))for i in range(240)]}
        pretty=(json.dumps(value,sort_keys=True,indent=2)+'\n').encode()
        raw=generator.encoded(value)
        self.assertGreater(len(pretty),256*1024)
        self.assertLessEqual(len(raw),256*1024)
        self.assertEqual(json.loads(raw),json.loads(pretty))
        files={consumer.INVENTORY:raw,path:full}
        self.assertEqual(consumer.validate_notice_inventory(files,required=True)['components'],240)
        with self.assertRaisesRegex(ValueError,'notice inventory budget'):
            consumer.validate_notice_inventory({**files,consumer.INVENTORY:pretty},required=True)
        with self.assertRaisesRegex(ValueError,'notice digest mismatch'):
            consumer.validate_notice_inventory({**files,path:full[:-1]},required=True)


class NormalBinding(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        sys.path.insert(0,str(DELIVERY))
        cls.module=load('cc_prometheus_notice_binding',DELIVERY/'prometheus_materials.py')
        sys.path.pop(0)
    def original(self):
        binary=bytearray(64);binary[:6]=b'\x7fELF\x02\x01';binary[18:20]=(62).to_bytes(2,'little')
        binary=bytes(binary);expected={'cmd/prometheus/main.go':'a'*64};mapping=encoded(expected)
        info=encoded({'GoVersion':'go1.27.4','Path':'github.com/prometheus/prometheus/cmd/prometheus',
            'Main':{'Path':'github.com/prometheus/prometheus','Version':'(devel)'},'Deps':[{'Path':'example.org/mod','Version':'v1.0.0','Sum':'h1:'+base64.b64encode(b'a'*32).decode()}]})
        report={'passed':True,'normalProductBuild':True,'sourceUnchanged':True,'noDaemonStarted':True,
            'binarySha256':digest(binary),'binaryBytes':len(binary),'sourceMapSha256':digest(mapping),'buildInfoSha256':digest(info)}
        return binary,report,mapping,expected,info
    def test_normal_descriptor_requires_exact_binary_source_map_and_reader_facts(self):
        b,r,m,e,i=self.original();observed=self.module.binding(b,r,m,e,i,i)
        self.assertEqual(observed['Main']['Version'],'(devel)') # Never invent3.16 embeddedversion.
        self.assertEqual(observed['GoVersion'],'go1.27.4')
    def test_test_binary_failed_compile_or_changed_source_never_becomes_component(self):
        b,r,m,e,i=self.original()
        for name,value in [('passed',False),('normalProductBuild',False),('sourceUnchanged',False),('binarySha256','0'*64),('binaryBytes',True)]:
            wrong=dict(r);wrong[name]=value
            with self.assertRaises(ValueError):self.module.binding(b,wrong,m,e,i,i)
        changed=encoded({'cmd/prometheus/main.go':'b'*64});r['sourceMapSha256']=digest(changed)
        with self.assertRaises(ValueError):self.module.binding(b,r,changed,e,i,i)
    def test_observed_test_path_foreign_main_recipe_toolchain_or_replacement_deny(self):
        b,r,m,e,i=self.original()
        for field,value in [('Path','github.com/prometheus/prometheus/tsdb/agent.test'),('GoVersion','go1.26.7'),
            ('Main',{'Path':'github.com/prometheus/prometheus','Version':'(devel)','Replace':{'Path':'local'}})]:
            changed=json.loads(i);changed[field]=value;raw=encoded(changed)
            with self.assertRaises(ValueError):self.module.binding(b,r,m,e,raw,raw)
    def test_recorded_raw_buildinfo_cannot_be_reformatted_or_swapped(self):
        b,r,m,e,i=self.original()
        formatted=json.dumps(json.loads(i),indent=2).encode()
        with self.assertRaises(ValueError):self.module.binding(b,r,m,e,formatted,i)
    def test_missing_or_partial_explicit_artifact_denies_without_any_file_read(self):
        def no_read(*_):raise AssertionError('input should deny first')
        for spec in [None,{}, {'path':'/fake/test-ELF'}]:
            with self.assertRaises(ValueError):self.module.component(Path('/agent'),spec,no_read,lambda *_:None)
    def material(self):
        root='example.org/mod@v1.0.0/';out=io.BytesIO()
        with zipfile.ZipFile(out,'w')as z:
            z.writestr(root+'LICENSE','Synthetic complete license text.\n')
            z.writestr(root+'NOTICE','Synthetic complete attribution.\n')
            z.writestr(root+'AUTHORS','') # Preserve actual empty source facts too.
        raw=out.getvalue();h1='h1:'+base64.b64encode(b'a'*32).decode()
        original={'Path':'example.org/mod','Version':'v1.0.0','Sum':h1}
        row={'path':'example.org/mod','version':'v1.0.0','sum':h1,'archivePath':'/fixture/public.zip','archiveSha256':digest(raw),
            'materials':self.module.go_materials.zip_materials(raw,'example.org/mod','v1.0.0')}
        return original,row,raw,{('example.org/mod','v1.0.0'):h1}
    def test_full_original_module_notices_and_empty_material_preserved(self):
        original,row,raw,sums=self.material()
        identity,full=self.module.checked_dependency(original,row,raw,sums)
        self.assertEqual(identity['sum'],original['Sum']);self.assertEqual(len(full),3)
        self.assertEqual(next(r['text']for r in full if r['path']=='AUTHORS'),'')
    def test_missing_notice_modified_text_zip_or_unpinned_h1_is_refusal(self):
        original,row,raw,sums=self.material()
        for mutate in [lambda r:r['materials'].pop(),lambda r:r['materials'][0].update(text='changed'),lambda r:r.update(archiveSha256='0'*64)]:
            wrong=copy.deepcopy(row);mutate(wrong)
            with self.assertRaises(ValueError):self.module.checked_dependency(original,wrong,raw,sums)
        with self.assertRaises(ValueError):self.module.checked_dependency(original,row,raw,{})
    def reader(self,callback):
        @contextmanager
        def fake(*_):
            with tempfile.TemporaryDirectory()as directory:
                root=Path(directory)
                yield root,lambda args,timeout=15:callback(root,args,timeout)
        return fake
    def test_official_verifier_gets_exact_zip_expected_h1_and_only_explicit_graph(self):
        _,row,raw,_=self.material();calls=[]
        def command(root,args,timeout):
            self.assertEqual(args,['mod','verify']);self.assertEqual(timeout,120)
            cache=root/'modcache/cache/download/example.org/mod/@v'
            self.assertEqual((cache/'v1.0.0.zip').read_bytes(),raw)
            self.assertEqual((cache/'v1.0.0.ziphash').read_text(),row['sum'])
            self.assertEqual((cache/'v1.0.0.mod').read_text(),'module example.org/mod\ngo 1.27.4\n')
            self.assertIn('example.org/mod v1.0.0', (root/'go.mod').read_text())
            self.assertFalse((root/'modcache/example.org').exists()) # No source extraction.
            calls.append(args);return b'all modules verified\n'
        with patch.object(self.module,'official_reader',self.reader(command)):
            self.module.verify_dependencies('/fixed/go',lambda *_:raw,'a'*64,[row])
        self.assertEqual(len(calls),1)
    def test_own_sha_of_modified_zip_cannot_bypass_official_h1_failure(self):
        _,row,raw,_=self.material();modified=raw+b'modified';row['archiveSha256']=digest(modified)
        def rejected(*_):raise subprocess.CalledProcessError(1,['fixed-go','mod','verify'])
        with patch.object(self.module,'official_reader',self.reader(rejected)):
            with self.assertRaises(subprocess.CalledProcessError):
                self.module.verify_dependencies('/fixed/go',lambda *_:modified,'a'*64,[row])
    def test_buildinfo_reader_sees_private_original_bytes_and_input_is_never_executed(self):
        binary,_,_,_,info=self.original()
        def command(root,args,timeout):
            self.assertEqual(args[:4],['version','-m','-json',str(root/'input.elf')])
            self.assertEqual((root/'input.elf').read_bytes(),binary)
            return info
        with patch.object(self.module,'official_reader',self.reader(command)):
            self.assertEqual(self.module.actual_build_info('/fixed/go',binary,lambda *_:None,'a'*64),info)

if __name__=='__main__':unittest.main()
