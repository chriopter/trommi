#!/usr/bin/env python3
"""A short coordinated frozen copy, then an offline SQLite backup and a private archive. Never prints data."""
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tarfile
import tempfile
import time


def docker(*args):
    return subprocess.check_output(['docker', *args], stderr=subprocess.DEVNULL, timeout=30, text=True)


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY)
    try: os.fsync(fd)
    finally: os.close(fd)


def resume_locked(recovery, command):
    marker = recovery / 'backup-paused.json'
    if not marker.exists(): return
    if marker.is_symlink(): raise RuntimeError('Invalid backup pause marker')
    record = json.loads(marker.read_text())
    name, identity = record['name'], record['container']
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*', name) or not re.fullmatch(r'[0-9a-f]{64}', identity):
        raise RuntimeError('Invalid backup container identity')
    current = json.loads(command('inspect', name))[0]
    if current['Id'] == identity and current['State'].get('Paused'):
        command('unpause', identity)
    if current['Id'] == identity and current['State'].get('Health'):
        deadline = time.monotonic() + 45
        while True:
            state = json.loads(command('inspect', name))[0]['State']
            if state.get('Health', {}).get('Status') == 'healthy': break
            if not state['Running'] or time.monotonic() >= deadline:
                raise RuntimeError('Hub resumed but its Docker healthcheck did not recover')
            time.sleep(0.5)
    marker.unlink()
    sync_directory(recovery)


def resume(recovery, command=docker):
    fd = os.open(recovery / 'operation.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        resume_locked(recovery, command)
    finally: os.close(fd)


def snapshot(container, data, config, root, recovery, command=docker):
    lock = os.open(recovery / 'operation.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        resume_locked(recovery, command)
        info = json.loads(command('inspect', container))[0]
        if not info['State']['Running'] or info['State'].get('Paused'):
            raise RuntimeError('Hub must be running and not already paused')
        identity = info['Id']
        marker = recovery / 'backup-paused.json'
        with marker.open('x') as f:
            json.dump({'name':container, 'container':identity}, f); f.flush(); os.fsync(f.fileno())
        sync_directory(recovery)
        try:
            command('pause', identity)
            # Copy SQLite DB+WAL and immutable files at the same frozen point. Compression runs after resume.
            for source, name in [(data, 'data'), (config, 'config')]:
                subprocess.run(['cp', '-a', '--reflink=auto', str(source), str(root / name)],
                               check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
            (root / 'snapshot.json').write_text(json.dumps({'image': info['Image'], 'created': int(time.time())}) + '\n')
        finally:
            resume_locked(recovery, command)
    finally:
        os.close(lock)
    # Consolidate the frozen WAL through SQLite's backup API, operating only on the private copied database.
    copied = root / 'data' / 'hub.db'
    normalized = root / 'data' / 'hub.db.backup'
    with contextlib.closing(sqlite3.connect(copied)) as source, contextlib.closing(sqlite3.connect(normalized)) as target:
        source.backup(target)
        if target.execute('PRAGMA quick_check').fetchone() != ('ok',):
            raise RuntimeError('Copied SQLite database failed integrity check')
    normalized.replace(copied)
    for suffix in ('-wal', '-shm'):
        copied.with_name(copied.name + suffix).unlink(missing_ok=True)


def backup(container, data, config, destination, recovery, command=docker):
    destination.mkdir(mode=0o700, parents=True, exist_ok=True)
    if destination.stat().st_mode & 0o077:
        raise RuntimeError('Backup directory must be private')
    stamp = time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())
    archive = destination / f'trommi-container-{stamp}.tgz'
    if archive.exists():
        raise RuntimeError('Backup already exists for this second')
    with tempfile.TemporaryDirectory(prefix='.trommi-snapshot-', dir=destination) as folder:
        root = Path(folder)
        snapshot(container, data, config, root, recovery, command)
        pending = root / 'archive.tgz'
        with tarfile.open(pending, 'w:gz') as tar:
            for name in ('data', 'config', 'snapshot.json'):
                tar.add(root / name, arcname=name)
        with tarfile.open(pending, 'r:gz') as tar:
            for member in tar:
                if member.isfile():
                    with tar.extractfile(member) as content:
                        while content.read(1 << 20): pass
        with pending.open('rb') as f:
            digest = hashlib.file_digest(f, 'sha256').hexdigest()
            os.fsync(f.fileno())
        pending.chmod(0o600)
        pending.replace(archive)
        checksum = archive.with_suffix(archive.suffix + '.sha256')
        checksum.write_text(f'{digest}  {archive.name}\n'); checksum.chmod(0o600)
        with checksum.open('rb') as f: os.fsync(f.fileno())
        fd = os.open(destination, os.O_RDONLY)
        try: os.fsync(fd)
        finally: os.close(fd)
    # Only this service's completed archives: never prune legacy backups or deployment recovery history.
    for old in destination.glob('trommi-container-????????T??????Z.tgz'):
        if old != archive and old.stat().st_mtime < time.time() - 14 * 86400:
            old.unlink(); old.with_suffix(old.suffix + '.sha256').unlink(missing_ok=True)
    print(f'Verified hub backup: {archive.name} ({archive.stat().st_size} bytes).')
    return archive


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(); parser.add_argument('--resume', action='store_true'); args = parser.parse_args()
    recovery = Path('/home/docker/yolodeploy/recovery/trommi')
    if args.resume:
        resume(recovery)
    else:
        backup('trommi', Path('/home/docker/trommi/data'), Path('/home/docker/trommi/config'),
               Path('/srv/trommi/backups/container'), recovery)
