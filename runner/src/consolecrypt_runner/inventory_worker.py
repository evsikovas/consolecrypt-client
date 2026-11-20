"""Fixed static-inventory reader in the same networkless job namespace."""
import base64
import hashlib
import os
from pathlib import Path
import stat
import sys

from .errors import RunnerError,require
from .inventory import MAX_INVENTORY_BYTES, merge_inventories, parse_ini_inventory, parse_yaml_inventory,apply_variable_sources,_name
from .protocol import decode, encode
from .snapshot import relative_path


def validate_sources(sources):
    require(isinstance(sources,list) and 0<len(sources)<=128,'inventory_sources','Expanded static inventory exceeds its file limit')
    seen=set();result=[]
    for source in sources:
        require(isinstance(source,dict) and set(source)=={'path','format'},'inventory_sources','Invalid static source selection')
        path=relative_path(source['path']).as_posix()
        require(path not in seen and source['format'] in ('ini','yaml','group_vars','host_vars'),'inventory_sources','Duplicate or unsupported static source')
        require(Path(path).suffix.lower() not in ('.py','.sh','.ps1','.exe','.bat','.cmd'),
                'inventory_script','Executable inventory sources are not enabled')
        seen.add(path);result.append({'path':path,'format':source['format']})
    return sorted(result,key=lambda source:source['path'])


def variable_target(path,scope):
    parts=relative_path(path).parts
    require(scope in ('group_vars','host_vars') and scope in parts,'inventory_vars_target','Invalid variable file scope')
    at=parts.index(scope)
    require(at+1<len(parts),'inventory_vars_target','Variable file target is missing')
    target=parts[at+1]
    if at+2==len(parts):
        for suffix in ('.yaml','.yml','.json'):
            if target.lower().endswith(suffix):target=target[:-len(suffix)];break
    return _name(target)


def read_source(root,entry):
    path=relative_path(entry['path'])
    require(type(entry['bytes']) is int and 0<=entry['bytes']<=MAX_INVENTORY_BYTES and
            isinstance(entry['sha256'],str) and len(entry['sha256'])==64,'inventory_source','Invalid captured source')
    directory=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:
        for part in path.parts[:-1]:
            child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=directory)
            os.close(directory);directory=child
        fd=os.open(path.parts[-1],os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=directory)
    finally:os.close(directory)
    with os.fdopen(fd,'rb') as source:
        info=os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_size==entry['bytes'],
                'inventory_changed','Static source differs from the captured project')
        body=source.read(entry['bytes']+1)
    require(len(body)==entry['bytes'] and hashlib.sha256(body).hexdigest()==entry['sha256'],
            'inventory_changed','Static source differs from the captured project')
    try:return body.decode('utf-8-sig')
    except UnicodeError:raise ValueError('Static inventory is not UTF-8')


def emit(value):
    sys.stdout.buffer.write(encode(value)+b'\n');sys.stdout.buffer.flush()


def main():
    from .worker_paths import SOURCE, tool
    specification=decode(tool('inventory-input.json').read_bytes())
    require(set(specification)=={'sources','entries'},'inventory_sources','Invalid trusted source input')
    sources=validate_sources(specification['sources'])
    entries=specification['entries']
    require(isinstance(entries,dict) and set(entries)=={source['path'] for source in sources},
            'inventory_source','Source files differ from the captured selection')
    require(sum(entry['bytes'] for entry in entries.values())<=MAX_INVENTORY_BYTES,
            'inventory_size','Combined inventory sources exceed the byte limit')
    parsed=[];variables=[]
    for source in sources:
        body=read_source(SOURCE,entries[source['path']])
        if source['format'] in ('group_vars','host_vars'):
            variables.append((source['path'],source['format'],variable_target(source['path'],source['format']),body))
            continue
        parser=parse_ini_inventory if source['format']=='ini' else parse_yaml_inventory
        inventory=parser(body,source['path'],resolve=False)
        # Preserve the digest of original bytes, including an optional BOM.
        inventory.source_digests[source['path']]=entries[source['path']]['sha256']
        parsed.append(inventory)
    require(parsed,'inventory_sources','Select at least one static inventory file')
    inventory=apply_variable_sources(merge_inventories(parsed,resolve=False),variables)
    inventory.source_digests={path:entry['sha256'] for path,entry in entries.items()}
    body=encode(inventory.to_snapshot())
    require(len(body)<=MAX_INVENTORY_BYTES,'inventory_size','Normalized inventory exceeds its byte limit')
    emit({'type':'inventory','bytes':len(body)})
    for offset in range(0,len(body),128*1024):
        emit({'type':'chunk','offset':offset,'data':base64.b64encode(body[offset:offset+128*1024]).decode('ascii')})
    emit({'type':'complete','sha256':hashlib.sha256(body).hexdigest()})


if __name__=='__main__':
    try:main()
    except RunnerError as error:emit({'type':'error','code':error.code,'message':str(error)})
    except Exception:emit({'type':'error','code':'inventory_parse','message':'Static inventory could not be read'})
