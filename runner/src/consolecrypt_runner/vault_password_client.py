"""Fixed executable client: private password pipe only, no resolver/CLI inputs.
The trusted wrapper supplies this executable path to Ansible, NEVER a password.
Not installed/exposed by any default capability in this stage.
"""
import base64,json,socket,sys
SOCKET='/run/consolecrypt/broker.sock'
def _main(socket_path):
    raw=bytearray();password=bytearray()
    try:
        if len(sys.argv)!=1:raise ValueError()
        with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as channel:
            channel.settimeout(6);channel.connect(socket_path)
            channel.sendall(b'{"version":1,"operation":"ansible_vault_password"}\n')
            while len(raw)<=8192:
                chunk=channel.recv(min(1024,8193-len(raw)))
                if not chunk:break
                raw.extend(chunk)
                if raw.endswith(b'\n'):break
        if len(raw)>8192 or not raw.endswith(b'\n'):raise ValueError()
        def unique(items):
            value={}
            for key,item in items:
                if key in value:raise ValueError()
                value[key]=item
            return value
        response=json.loads(raw,object_pairs_hook=unique)
        if set(response)!={'version','result','error'} or type(response['version']) is not int or response['version']!=1 or response['error'] is not None:raise ValueError()
        result=response['result']
        if not isinstance(result,dict) or set(result)!={'password_base64'} or not isinstance(result['password_base64'],str) or not 0<len(result['password_base64'])<=5464:raise ValueError()
        password.extend(base64.b64decode(result['password_base64'],validate=True))
        if not 2<=len(password)<=4097 or password[-1:]!=b'\n':raise ValueError()
        value=password[:-1].decode('utf-8')
        if value.strip()!=value or any(c<' ' or c=='\x7f' or c in '\u0085\u2028\u2029' or '\u0080'<=c<='\u009f' for c in value):raise ValueError()
        # stdout MUST be the PRIVATE Ansible password pipe. Never call this
        # client as a console command or route its output through diagnostics.
        sys.stdout.buffer.write(password);sys.stdout.buffer.flush()
        return 0
    except (OSError,ValueError,UnicodeError,TypeError,KeyError,RecursionError):
        # No exception repr, response body, diagnostic or password on stderr.
        return 65
    finally:
        raw[:]=b'\x00'*len(raw);password[:]=b'\x00'*len(password)
def main():return _main(SOCKET)
def main_job():return _main('/run/consolecrypt-vault/broker.sock')
if __name__=='__main__':raise SystemExit(main())
