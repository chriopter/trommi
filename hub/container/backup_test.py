import json
from pathlib import Path
import sqlite3
import tarfile
import tempfile
import unittest
import backup


class Tests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.data = self.root / 'live'; self.data.mkdir()
        self.config = self.root / 'config'; self.config.mkdir()
        (self.config / 'fixture.env').write_text('FIXTURE=only\n')
        self.db = sqlite3.connect(self.data / 'hub.db'); self.addCleanup(self.db.close)
        self.db.execute('PRAGMA journal_mode=WAL'); self.db.execute('PRAGMA wal_autocheckpoint=0')
        self.db.execute('CREATE TABLE fixture (value TEXT)'); self.db.execute("INSERT INTO fixture VALUES ('committed WAL')"); self.db.commit()
        (self.data / 'files').mkdir(); (self.data / 'files' / 'object').write_bytes(b'fixture object')
        self.recovery = self.root / 'recovery'; self.recovery.mkdir(mode=0o700)
        self.calls = []; self.paused = False
    def command(self, *args):
        self.calls.append(args)
        if args[0] == 'pause': self.paused = True
        if args[0] == 'unpause': self.paused = False
        return json.dumps([{'Id':'a' * 64,'Image':'sha256:fixture','State':{'Running':True,'Paused':self.paused}}]) if args[0] == 'inspect' else ''
    def test_wal_files_and_config_restore_from_a_verified_private_archive(self):
        archive = backup.backup('fixture', self.data, self.config, self.root / 'backups', self.recovery, self.command)
        restore = self.root / 'restored'; restore.mkdir()
        with tarfile.open(archive) as tar: tar.extractall(restore, filter='data')
        with sqlite3.connect(restore / 'data/hub.db') as db:
            self.assertEqual(db.execute('SELECT value FROM fixture').fetchall(), [('committed WAL',)])
        self.assertEqual((restore / 'data/files/object').read_bytes(), b'fixture object')
        self.assertEqual((restore / 'config/fixture.env').read_text(), 'FIXTURE=only\n')
        self.assertEqual(archive.stat().st_mode & 0o777, 0o600)
        self.assertIn(('pause','a' * 64), self.calls)
        self.assertEqual(self.calls[-1], ('unpause','a' * 64))
        self.assertTrue(archive.with_suffix('.tgz.sha256').is_file())
    def test_failed_copy_always_resumes_the_hub_and_publishes_no_archive(self):
        with self.assertRaises(Exception):
            backup.backup('fixture', self.root / 'missing', self.config, self.root / 'backups', self.recovery, self.command)
        self.assertEqual(self.calls[-1], ('unpause','a' * 64))
        self.assertFalse(list((self.root / 'backups').glob('*.tgz')))
    def test_an_already_paused_hub_is_left_alone(self):
        def paused(*args):
            return json.dumps([{'Id':'fixture','Image':'fixture','State':{'Running':True,'Paused':True}}])
        with self.assertRaisesRegex(RuntimeError, 'already paused'):
            backup.backup('fixture', self.data, self.config, self.root / 'backups', self.recovery, paused)
        self.assertEqual(self.calls, [])

    def test_interrupted_snapshot_can_resume_only_its_own_container(self):
        marker = self.recovery / 'backup-paused.json'
        marker.write_text(json.dumps({'name':'fixture', 'container':'a' * 64}))
        self.paused = True
        backup.resume(self.recovery, self.command)
        self.assertFalse(self.paused); self.assertFalse(marker.exists())
        marker.write_text(json.dumps({'name':'fixture', 'container':'b' * 64}))
        self.calls.clear(); self.paused = True
        backup.resume(self.recovery, self.command)
        self.assertTrue(self.paused)
        self.assertFalse(any(c[0] == 'unpause' for c in self.calls))

if __name__ == '__main__': unittest.main()
