#!/usr/bin/env python3
"""Verify a package ZIP and drive its extracted executable offline; never installs it.

The fake host grants only the manifest's declared capabilities and refuses every
reverse request, so a run must not touch the desktop, clipboard, browser or network.
`--open COMMAND[=MESSAGE]` (repeatable, in order) opens a session, checks its
mode against the manifest and optionally its no-view message, then disposes it.
"""
import argparse, hashlib, json, os, platform, queue, stat, subprocess, tempfile, threading, uuid, zipfile
from pathlib import Path, PurePosixPath

MAX_FRAME = 16 * 1024 * 1024


def smoke(path, opens, legacy_counter=None):
    with tempfile.TemporaryDirectory() as temp, zipfile.ZipFile(path) as archive:
        root = Path(temp) / 'package'
        root.mkdir()
        names = archive.namelist()
        assert len(names) == len(set(names)), 'Duplicate ZIP paths'
        for item in archive.infolist():
            name = PurePosixPath(item.filename)
            assert not name.is_absolute() and '..' not in name.parts and '\\' not in item.filename and ':' not in item.filename, 'Unsafe ZIP path'
            assert not stat.S_ISLNK(item.external_attr >> 16), 'Symlink ZIP entry'
        manifest = json.loads(archive.read('extension.json'))
        inventory = manifest['files']
        assert set(names) == set(inventory) | {'extension.json'}, 'ZIP/inventory mismatch'
        for name, digest in inventory.items():
            assert hashlib.sha256(archive.read(name)).hexdigest() == digest, 'SHA inventory mismatch: ' + name
        notices = json.loads(archive.read('licenses/dependency-license-index.json'))
        assert notices['targets'] == ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']
        sdk = next(e for e in notices['dependencies'] if e['name'] == 'tezbar-extension-sdk')
        assert sdk['source'].endswith('#aaec571673ab64a85a136cf6bd054ef660c4d93c')
        assert {PurePosixPath(n).name for n in sdk['documents']} >= {'LICENSE', 'NOTICE'}
        assert any(e['name'] == 'tokio' for e in notices['dependencies'])
        for entry in notices['dependencies']:
            assert entry['targets'] and set(entry['targets']) <= set(notices['targets'])
            for name, digest in entry['documents'].items():
                assert name.startswith('licenses/crates/') and '\\' not in name
                assert inventory[name] == digest
                assert hashlib.sha256(archive.read(name)).hexdigest() == digest
        assert manifest['schemaVersion'] == 3 and manifest['execution'] == 'process' and manifest['protocolVersion'] == 1
        assert manifest['id'] == 'com.tezbar.native.' + manifest['slug']
        assert not manifest.get('icon') or manifest['icon'] in inventory, 'Icon not packaged'
        archive.extractall(root)
        slug = manifest['slug']
        binary = root / 'bin' / ('tezbar-' + slug + ('.exe' if os.name == 'nt' else ''))
        if os.name != 'nt':
            assert os.stat(binary).st_mode & 0o111 or archive.getinfo(binary.relative_to(root).as_posix()).external_attr >> 16 & 0o111, 'Executable bit missing'
            binary.chmod(0o755)
        env = {k: v for k, v in os.environ.items() if k in ('SystemRoot', 'WINDIR', 'TEMP', 'TMP')}
        env['PATH'] = os.path.join(env.get('SystemRoot', 'C:/Windows'), 'System32') if os.name == 'nt' else '/usr/bin:/bin'
        info = json.loads(subprocess.check_output([str(binary), '--protocol-info'], cwd=root, env=env, timeout=10))
        assert info['id'] == manifest['id'] and info['version'] == manifest['version'] and info['protocol'] == 1
        modes = {c['name']: c.get('mode') for c in manifest['commands']}
        assert set(info['commands']) == {c['name'] for c in manifest['commands'] if c['runtime'] == 'rust'}
        process = subprocess.Popen([str(binary)], cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=None)
        messages = queue.Queue()
        reverse = []

        def reader():
            try:
                while True:
                    line = process.stdout.readline(MAX_FRAME + 1)
                    if not line:
                        messages.put(EOFError('Unexpected EOF'))
                        return
                    if len(line) > MAX_FRAME:
                        raise ValueError('Oversized frame')
                    messages.put(json.loads(line))
            except Exception as error:
                messages.put(error)

        threading.Thread(target=reader, daemon=True).start()

        def send(value):
            process.stdin.write((json.dumps(value) + '\n').encode())
            process.stdin.flush()

        def receive():
            value = messages.get(timeout=8)
            if isinstance(value, Exception):
                raise value
            return value

        def request(method, params):
            ident = 'h:' + str(uuid.uuid4())
            send({'type': 'request', 'id': ident, 'method': method, 'params': params})
            while True:
                frame = receive()
                if frame['type'] == 'response' and frame['id'] == ident:
                    assert 'error' not in frame, frame
                    return frame['result']
                if frame['type'] == 'request':
                    reverse.append(frame)
                    send({'type': 'response', 'id': frame['id'], 'error': {'code': 'capability_denied', 'message': 'smoke host refuses reverse requests'}})
                    continue
                assert frame['type'] == 'event', frame

        data = Path(temp) / 'data'
        migrations = {}
        legacy = None
        if legacy_counter is not None:
            relative = manifest['migrationPaths']['legacyRaycastStorage']
            assert relative == 'extensions/raycast.counter/.tezbar-local-storage.json'
            legacy = Path(temp) / 'profile' / relative
            legacy.parent.mkdir(parents=True)
            original = json.dumps({'counter': str(legacy_counter)}).encode()
            legacy.write_bytes(original)
            migrations['legacyRaycastStorage'] = str(legacy)
        arch = {'arm64': 'aarch64', 'AMD64': 'x86_64'}.get(platform.machine(), platform.machine())
        triple = arch + ('-pc-windows-msvc' if os.name == 'nt' else '-apple-darwin')
        try:
            hello = receive()
            assert hello['type'] == 'hello' and hello['id'] == manifest['id'] and hello['version'] == manifest['version'] and hello['protocol'] == 1
            assert hello['sdkVersion'] == '0.1.1' and 'launcher.v1' in hello['features']
            ready = request('initialize', {'protocol': 1, 'extensionId': manifest['id'], 'platform': triple,
                                           'packageDir': str(root), 'dataDir': str(data), 'tempDir': str(Path(temp) / 'temp'),
                                           'grantedCapabilities': manifest['capabilities'], 'reason': 'command', 'migrationPaths': migrations, 'hostFeatures': ['launcher.v1']})
            assert ready.get('ready') is True and 'launcher.v1' in ready['features'], ready
            for index, spec in enumerate(opens or [manifest['commands'][0]['name']]):
                command, _, message = spec.partition('=')
                session = f'zip-smoke-{index}'
                result = request('session.open', {'sessionId': session, 'commandName': command, 'preferences': {}, 'argumentValues': {}})
                assert result['ok'] is True and result['mode'] == modes[command], (command, result)
                # native.rs leaves no-view success intact; CommandBar consumes message.
                assert result['mode'] == 'no-view' and isinstance(result.get('message'), str) and result['message'], result
                if message:
                    assert result.get('message') == message, (command, result)
                request('session.dispose', {'sessionId': session})
                print(f'  {command}: mode={result["mode"]} message={result.get("message")!r}')
            if legacy is not None:
                assert legacy.read_bytes() == original, 'Legacy source modified'
                store = json.loads((data / 'counter/storage.json').read_text())
                assert store['_tezbarMigration']['source'] == 'legacyRaycastStorage'
            request('shutdown', {})
            assert process.wait(timeout=5) == 0, 'Process did not exit cleanly'
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stdin.close()
            process.stdout.close()
        assert not reverse, f'Unexpected reverse host requests: {reverse}'
        print(slug, 'ZIP inventory + protocol-info/hello/initialize/open/dispose/shutdown passed (offline, no installation)')


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('archive', type=Path)
    p.add_argument('--open', action='append', default=[], metavar='COMMAND[=MESSAGE]')
    p.add_argument('--legacy-counter', type=int, help='Seed the real extensions/raycast.counter migration layout')
    a = p.parse_args()
    smoke(a.archive, a.open, a.legacy_counter)
