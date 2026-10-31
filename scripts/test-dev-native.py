#!/usr/bin/env python3
"""Hermetic public-CLI acceptance for dev-native.sh (no GPU/public network).

Run: python3 scripts/test-dev-native.py
The real-host/GPU compatibility check remains scripts/dev-native.sh --smoke.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parent.parent
INPUTS = ["crates/mmorpg-client/src/main.rs", "crates/mmorpg-client/src/hud.wgsl",
          "crates/mmorpg-scenery/assets/outpost-grass/selected.props.rs",
          "content/greyhaven/outpost-layout.json", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml"]
FAKE = r'''#!/usr/bin/env python3
import hashlib,json,os,pathlib,signal,subprocess,sys,time
root=pathlib.Path(os.environ['FIXTURE_ROOT']); control=pathlib.Path(os.environ['CONTROL'])
def record(kind, **extra):
 with (control/'events').open('a') as f: f.write(json.dumps(dict(kind=kind,pid=os.getpid(),cwd=os.getcwd(),args=sys.argv[1:],**extra))+'\n')
def fingerprint():
 return hashlib.sha256(b''.join(p.relative_to(root).as_posix().encode()+p.read_bytes() for p in sorted(root.rglob('*')) if p.is_file() and '.git' not in p.parts and 'target' not in p.parts)).hexdigest()
name=pathlib.Path(sys.argv[0]).name
if name=='curl': sys.exit(1 if (control/'unready').exists() else 0)
if name=='openssl':
 if '-checkend' in sys.argv: sys.exit(0 if '86400' in sys.argv else 1)
 if '-text' in sys.argv: print('prime256v1')
 for flag in ['-keyout','-out']:
  if flag in sys.argv: pathlib.Path(sys.argv[sys.argv.index(flag)+1]).write_text('fixture')
 sys.exit(0)
if name=='cargo' and 'metadata' in sys.argv:
 print(json.dumps({'target_directory':str(root/'target')})); sys.exit(0)
if name=='cargo' and 'build' in sys.argv:
 captured=fingerprint(); record('build',fingerprint=captured)
 while (control/'block-build').exists(): time.sleep(.03)
 if (control/'fail-build').exists(): print('FIXTURE BUILD FAILED',file=sys.stderr); sys.exit(42)
 for binary in ['mmorpg-client','mmorpg-zone-host']:
  p=root/'target/debug'/binary; p.parent.mkdir(parents=True,exist_ok=True); p.write_text(pathlib.Path(__file__).read_text()); p.chmod(0o755)
  if any(a.startswith('--message-format') for a in sys.argv):
   print(json.dumps({'reason':'compiler-artifact','target':{'name':binary,'kind':['bin']},'executable':str(p)}))
 (root/'target/build-fingerprint').write_text(captured)
 record('built',fingerprint=captured); sys.exit(0)
if '--fixture-worker' in sys.argv:
 record('worker')
 while True: time.sleep(.03)
host=name=='mmorpg-zone-host' or 'mmorpg-game-server' in sys.argv
kind='host' if host else 'client'
record(kind,fingerprint=(root/'target/build-fingerprint').read_text())
def stop(*unused):
 record(kind+'-stop'); sys.exit(0)
signal.signal(signal.SIGTERM,stop); signal.signal(signal.SIGINT,stop)
if host and (control/'fail-host').exists(): sys.exit(43)
if host: subprocess.Popen([sys.executable,__file__,'--fixture-worker'],env=os.environ)
if not host and '--smoke' in sys.argv: sys.exit(0)
while True:
 if not host and (control/'close-client').exists(): sys.exit(0)
 time.sleep(.03)
'''


class LauncherAcceptance(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='native-launcher-')
        self.base = Path(self.temporary.name)
        self.origin = self.base/'origin.git'
        self.seed = self.base/'seed'
        self.repo = self.base/'checkout'
        self.control = self.base/'control'; self.control.mkdir()
        self.bin = self.base/'bin'; self.bin.mkdir()
        self.git(self.base, 'init', '--bare', str(self.origin))
        self.git(self.base, 'init', '-b', 'main', str(self.seed))
        self.git(self.seed, 'config', 'user.email', 'fixture@example.invalid')
        self.git(self.seed, 'config', 'user.name', 'Fixture')
        shutil.copytree(ROOT/'scripts', self.seed/'scripts')
        (self.seed/'.gitignore').write_text('target/\n')
        for name in INPUTS:
            path = self.seed/name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text('initial\n')
        self.git(self.seed, 'add', '.'); self.git(self.seed, 'commit', '-m', 'fixture')
        self.git(self.seed, 'remote', 'add', 'origin', str(self.origin))
        self.git(self.seed, 'push', '-u', 'origin', 'main')
        self.git(self.origin, 'symbolic-ref', 'HEAD', 'refs/heads/main')
        self.git(self.base, 'clone', str(self.origin), str(self.repo))
        self.git(self.repo, 'config', 'user.email', 'fixture@example.invalid')
        self.git(self.repo, 'config', 'user.name', 'Fixture')
        for command in ['cargo', 'curl', 'openssl']:
            path = self.bin/command; path.write_text(FAKE); path.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin)+os.pathsep+os.environ['PATH'],
                        FIXTURE_ROOT=str(self.repo), CONTROL=str(self.control), CARGO_TARGET_DIR=str(self.repo/'target'))
        self.process = None
        self.output = self.base/'output'

    def tearDown(self):
        if self.process and self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGKILL); self.process.wait(timeout=5)
        for event in self.events():
            if event['kind'] in ['host','client','worker']:
                try: os.kill(event['pid'], signal.SIGKILL)
                except ProcessLookupError: pass
        self.temporary.cleanup()

    def git(self, cwd, *args):
        return subprocess.check_output(['git', '-C', str(cwd), *args], stderr=subprocess.STDOUT, text=True).strip()

    def launch(self, *args):
        with self.output.open('w') as output:
            self.process = subprocess.Popen([str(self.repo/'scripts/dev-native.sh'), *args], cwd=self.base,
                                            env=self.env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)

    def events(self, kind=None):
        path = self.control/'events'
        entries = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return [entry for entry in entries if kind is None or entry['kind'] == kind]

    def wait(self, predicate, description, timeout=8):
        deadline = time.monotonic()+timeout
        while time.monotonic() < deadline:
            if predicate(): return
            if self.process and self.process.poll() not in [None, 0]:
                self.fail(description+' (launcher exited early)\n'+self.output.read_text())
            time.sleep(.04)
        self.fail(description+'\n'+self.output.read_text())

    def running(self, *args):
        self.launch(*args)
        self.wait(lambda: bool(self.events('client')), 'client must start')

    def stop(self):
        self.process.send_signal(signal.SIGINT)
        self.process.wait(timeout=5)
        self.wait(lambda: all(not self.alive(e['pid']) for e in self.events() if e['kind'] in ['host','client','worker']), 'owned processes must stop')

    @staticmethod
    def alive(pid):
        try:
            os.kill(pid, 0)
            stat = Path(f'/proc/{pid}/stat')
            return not stat.exists() or stat.read_text().split()[2] != 'Z'
        except ProcessLookupError: return False

    def edit(self, name, text='changed\n'):
        path = self.repo/name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(text)

    def test_fast_forward_before_build_and_external_cwd(self):
        (self.seed/'merged.txt').write_text('new merged source')
        self.git(self.seed,'add','.'); self.git(self.seed,'commit','-m','merged'); self.git(self.seed,'push')
        expected = self.git(self.seed,'rev-parse','HEAD')
        self.running('--no-watch')
        self.assertEqual(self.git(self.repo,'rev-parse','HEAD'), expected)
        self.assertTrue(all(e['cwd']==str(self.repo) for e in self.events()))
        self.assertTrue(all('--locked' in e['args'] for e in self.events('build')))
        self.stop()

    def test_current_checkout_preserves_dirty_and_untracked_work(self):
        self.edit(INPUTS[0], 'local edits\n'); self.edit('personal-note','keep\n')
        before = self.git(self.repo,'status','--porcelain')
        self.running('--no-watch')
        self.assertEqual(self.git(self.repo,'status','--porcelain'),before)
        self.assertEqual((self.repo/INPUTS[0]).read_text(),'local edits\n'); self.stop()

    def test_unsafe_updates_fail_without_build_or_mutation(self):
        for condition in ['dirty','diverged','offline']:
            with self.subTest(condition=condition):
                # Each unsafe state is independently derived from current local HEAD.
                if condition=='dirty':
                    self.edit(INPUTS[0],'uncommitted\n')
                    (self.seed/INPUTS[0]).write_text('merged\n')
                    self.git(self.seed,'add','.'); self.git(self.seed,'commit','-m','remote'); self.git(self.seed,'push')
                elif condition=='diverged':
                    self.git(self.repo,'add','.'); self.git(self.repo,'commit','-m','local')
                else: self.git(self.repo,'remote','set-url','origin',str(self.base/'missing.git'))
                head=self.git(self.repo,'rev-parse','HEAD'); status=self.git(self.repo,'status','--porcelain')
                self.launch('--no-watch'); self.assertNotEqual(self.process.wait(timeout=8),0)
                self.assertFalse(self.events('build'))
                self.assertEqual(self.git(self.repo,'rev-parse','HEAD'),head)
                self.assertEqual(self.git(self.repo,'status','--porcelain'),status)
                self.assertIn('--no-update',self.output.read_text())

    def test_smoke_stays_one_shot(self):
        self.launch('--smoke')
        self.assertEqual(self.process.wait(timeout=8),0,self.output.read_text())
        self.assertEqual(len(self.events('build')),1)
        self.assertEqual(len(self.events('client')),1)
        self.assertTrue(any('--smoke' in e['args'] for e in self.events('client')))
        self.wait(lambda: all(not self.alive(e['pid']) for e in self.events() if e['kind'] in ['host','worker']), 'one-shot host cleanup')

    def test_explicit_offline_one_shot_smoke(self):
        self.git(self.repo,'remote','set-url','origin',str(self.base/'missing.git'))
        self.launch('--no-update','--smoke')
        self.assertEqual(self.process.wait(timeout=8),0,self.output.read_text())
        self.assertEqual(len(self.events('build')),1)
        self.assertTrue(any('--smoke' in e['args'] for e in self.events('client')))
        self.wait(lambda: all(not self.alive(e['pid']) for e in self.events() if e['kind'] in ['host','worker']), 'smoke host cleanup')

    def test_watch_inputs_add_delete_and_ignore_outputs(self):
        self.running('--no-update')
        for index,name in enumerate(INPUTS+['crates/mmorpg-client/src/new.rs']):
            count=len(self.events('client')); self.edit(name,str(index))
            self.wait(lambda:len(self.events('client'))>count,'input must restart: '+name)
        path=self.repo/'crates/mmorpg-client/src/new.rs'; count=len(self.events('client')); path.unlink()
        self.wait(lambda:len(self.events('client'))>count,'deletion must restart')
        count=len(self.events('build'))
        self.edit('target/dev-native/zone-host.log','output'); self.edit('target/dev-tls/key.pem','output')
        time.sleep(1.5); self.assertEqual(len(self.events('build')),count)
        self.stop()

    def test_embedded_assets_restart_and_generated_web_outputs_are_ignored(self):
        """Fake Cargo proves watch classification; actual native smoke proves compilation.

        Missing include_bytes!/include_str! inputs may fail a real build. Simulate
        that failure and recover after restoration, without promising a missing
        asset can produce a valid native executable.
        """
        assets = [f"web/assets/medieval-character-kit/{name}"
                  for name in ['archer.obj', 'manifest.json', 'materials.json']]
        originals = {}
        for name in assets:
            path = self.repo/name; path.parent.mkdir(parents=True, exist_ok=True)
            originals[name] = (ROOT/name).read_bytes(); path.write_bytes(originals[name])
        self.running('--no-update')
        for name in assets:
            count = len(self.events('client'))
            (self.repo/name).write_bytes(originals[name]+b'\n')
            self.wait(lambda: len(self.events('client')) > count, 'embedded asset edit must restart: '+name)
            failures = self.output.read_text().count('FIXTURE BUILD FAILED')
            (self.control/'fail-build').touch()
            (self.repo/name).unlink()
            self.wait(lambda: self.output.read_text().count('FIXTURE BUILD FAILED') > failures,
                      'embedded asset deletion must rebuild and report failure: '+name)
            self.assertIsNone(self.process.poll(), 'missing asset must remain recoverable')
            count = len(self.events('client'))
            (self.control/'fail-build').unlink()
            (self.repo/name).write_bytes(originals[name])
            self.wait(lambda: len(self.events('client')) > count, 'restored embedded asset must restart: '+name)
        builds = len(self.events('build')); clients = len(self.events('client'))
        for name in ['web/dist/bundle.js', 'web/dist/style.css',
                     'web/src/generated/mmorpg-wasm/mmorpg_wasm.js', 'web/node_modules/example/index.js']:
            self.edit(name, 'generated output')
        time.sleep(1.5)
        self.assertEqual(len(self.events('build')), builds)
        self.assertEqual(len(self.events('client')), clients)
        self.stop()

    def test_no_watch_and_window_close_cleanup(self):
        self.running('--no-update','--no-watch'); self.edit(INPUTS[0]); time.sleep(1.2)
        self.assertEqual(len(self.events('build')),1)
        (self.control/'close-client').touch(); self.assertEqual(self.process.wait(timeout=5),0)
        self.wait(lambda: all(not self.alive(e['pid']) for e in self.events() if e['kind'] in ['host','worker']), 'window close host cleanup')

    def test_failed_rebuild_recovers_after_fix(self):
        self.running('--no-update'); (self.control/'fail-build').touch(); self.edit(INPUTS[0],'broken')
        self.wait(lambda:'FIXTURE BUILD FAILED' in self.output.read_text(),'build failure visible')
        self.assertIsNone(self.process.poll(),'watch must remain recoverable')
        count=len(self.events('client')); (self.control/'fail-build').unlink(); self.edit(INPUTS[0],'fixed')
        self.wait(lambda:len(self.events('client'))>count,'fixed source restarts client'); self.stop()

    def test_edit_during_build_is_not_lost(self):
        self.running('--no-update'); (self.control/'block-build').touch(); self.edit(INPUTS[0],'first')
        self.wait(lambda:len(self.events('build'))>=2,'rebuild starts')
        self.edit(INPUTS[1],'second edit while building'); (self.control/'block-build').unlink()
        self.wait(lambda:len(self.events('client'))>=2,'new client starts')
        expected=hashlib.sha256(b''.join(p.relative_to(self.repo).as_posix().encode()+p.read_bytes() for p in sorted(self.repo.rglob('*')) if p.is_file() and '.git' not in p.parts and 'target' not in p.parts)).hexdigest()
        self.wait(lambda:bool(self.events('client')) and self.events('client')[-1]['fingerprint']==expected,
                  'running client must contain the edit made during build')
        self.stop()

    def test_startup_failure_cleans_owned_processes(self):
        (self.control/'fail-host').touch(); (self.control/'unready').touch()
        self.launch('--no-update','--no-watch'); self.assertNotEqual(self.process.wait(timeout=8),0)
        self.assertFalse(self.events('client'))
        self.assertTrue(all(not self.alive(e['pid']) for e in self.events('host')))


if __name__=='__main__':
    unittest.main(verbosity=2)
