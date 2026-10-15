"""Generate in CI only; export the public key and a sealed GitHub Actions-secret ciphertext, never the private key."""
import base64
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import subprocess


def main():
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('GITHUB_REPOSITORY') != 'chriopter/trommi':
        raise RuntimeError('This bootstrap runs only inside this repository CI')
    recipient = json.loads(Path('release/hub-key-recipient.json').read_text())
    public = base64.b64decode(recipient['key'], validate=True)
    if len(public) != 32:
        raise RuntimeError('Invalid repository encryption public key')
    sodium = ctypes.CDLL(ctypes.util.find_library('sodium'))
    if sodium.sodium_init() < 0:
        raise RuntimeError('Encryption library unavailable')
    sodium.crypto_box_seal.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulonglong, ctypes.c_void_p]
    sodium.crypto_box_seal.restype = ctypes.c_int
    temp = Path(os.environ['RUNNER_TEMP'])
    private_dir = temp / 'hub-key-private'; private_dir.mkdir(mode=0o700)
    output = temp / 'hub-key-public'; output.mkdir(mode=0o700)
    key = private_dir / 'hub-cosign.key'
    try:
        subprocess.run([str(temp / 'cosign'), 'generate-key-pair', '--output-key-prefix', str(private_dir / 'hub-cosign')],
                       env={**os.environ, 'COSIGN_PASSWORD': ''}, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        secret = key.read_bytes()
        cipher = ctypes.create_string_buffer(len(secret) + 48)
        if sodium.crypto_box_seal(cipher, secret, len(secret), public) != 0:
            raise RuntimeError('Repository encryption failed')
        (output / 'encrypted-secret.json').write_text(json.dumps({'key_id': recipient['key_id'], 'encrypted_value': base64.b64encode(cipher.raw).decode()}) + '\n')
        (output / 'hub-cosign.pub').write_bytes((private_dir / 'hub-cosign.pub').read_bytes())
        print('Created a CI-only Cosign key. Only repository-encrypted ciphertext and the public key are exported.')
    finally:
        if key.exists(): key.unlink()


if __name__ == '__main__':
    main()
