"""Fixed trusted entrypoint; no CLI/env-controlled socket, ID or password.
Reuse the existing bounded private client under isolated Python path policy.
"""
import runpy
if __name__=='__main__':
    client=runpy.run_path('/tool/python/consolecrypt_runner/vault_password_client.py',run_name='consolecrypt_vault_client')
    raise SystemExit(client['main_job']())
