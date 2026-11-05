#!/usr/bin/env python3
"""Build the frozen root-layout ZIP, SHA inventory and sidecars. Never uploads."""
import argparse, hashlib, json, re, shutil, stat, subprocess, sys, tempfile, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {'aarch64-apple-darwin': False, 'x86_64-pc-windows-msvc': True}
PACKAGED_DIRS = ['resources', 'licenses']
PACKAGED_FILES = ['LICENSE', 'NOTICE']


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check_sdk():
    text = (ROOT / 'Cargo.toml').read_text()
    match = re.search(r'^tezbar-extension-sdk\s*=\s*\{([^}\n]*)\}', text, re.M)
    fields = match.group(1) if match else ''
    revision = re.search(r'\brev\s*=\s*"([0-9a-fA-F]{40})"', fields)
    if not revision or re.search(r'\b(path|branch|tag)\s*=', fields) or not re.search(r'\bgit\s*=\s*"https://github.com/[^" ]+"', fields):
        raise SystemExit('Release blocked: the SDK must be pinned to an immutable git revision.')
    lock = (ROOT / 'Cargo.lock').read_text()
    entry = re.search(r'\[\[package\]\]\nname = "tezbar-extension-sdk"(.*?)(?=\[\[package\]\]|\Z)', lock, re.S)
    if not entry or not re.search(r'source = "git\+[^"\n]+#' + revision.group(1) + r'"', entry.group(1)):
        raise SystemExit('Release blocked: Cargo.lock must contain the same immutable SDK git revision.')


def build(target, binary, output):
    check_sdk()
    # Fail before writing any ZIP if the locked two-target notice inventory is stale.
    subprocess.run([sys.executable, "-I", str(ROOT / "scripts/vendor-licenses.py"), "--check"], cwd=ROOT, check=True)
    manifest = json.loads((ROOT / 'extension.json').read_text())
    slug = manifest['slug']
    windows = TARGETS[target]
    executable = f'bin/tezbar-{slug}' + ('.exe' if windows else '')
    info = json.loads(subprocess.check_output([str(binary.resolve()), '--protocol-info'], timeout=10))
    assert info['id'] == manifest['id'] and info['version'] == manifest['version'] and info['protocol'] == 1, 'Binary identity mismatch'
    platform = 'Windows' if windows else 'macOS'
    expected = [c['name'] for c in manifest['commands'] if c['runtime'] == 'rust' and platform in c['platforms']]
    assert set(info['commands']) == set(expected), 'Binary/manifest platform coverage mismatch'
    icon = manifest.get('icon')
    assert not icon or (ROOT / icon).is_file() and icon.split('/')[0] in PACKAGED_DIRS, 'Manifest icon must be a packaged file'
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        stage = Path(temp)
        (stage / 'bin').mkdir()
        shutil.copy2(binary, stage / executable)
        for name in PACKAGED_DIRS:
            shutil.copytree(ROOT / name, stage / name)
        for name in PACKAGED_FILES:
            shutil.copy2(ROOT / name, stage / name)
        files = {}
        for path in sorted(stage.rglob('*')):
            if path.is_symlink():
                raise ValueError('Symlinks cannot be packaged')
            if path.is_file():
                files[path.relative_to(stage).as_posix()] = digest(path)
        # Recheck copied bytes too, before opening the ZIP (source verification
        # happened before staging). Never ship a notice with a stale indexed hash.
        notices = json.loads((stage / 'licenses/dependency-license-index.json').read_text())
        for dependency in notices['dependencies']:
            for name, expected_hash in dependency['documents'].items():
                assert files.get(name) == expected_hash, 'Staged notice mismatch: ' + name
        manifest['files'] = files
        # The parent supplies published archive URLs/digests in the trusted host catalog.
        manifest['artifacts'] = {}
        (stage / 'extension.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
        archive = output / f'{slug}-{target}.zip'
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as z:
            for path in sorted(stage.rglob('*')):
                if path.is_file():
                    name = path.relative_to(stage).as_posix()
                    entry = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
                    entry.create_system = 3
                    entry.external_attr = (stat.S_IFREG | (0o755 if name == executable else 0o644)) << 16
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    z.writestr(entry, path.read_bytes())
        with zipfile.ZipFile(archive) as z:
            packaged = json.loads(z.read('extension.json'))
            assert set(z.namelist()) == set(files) | {'extension.json'}
            for name, sha in packaged['files'].items():
                assert hashlib.sha256(z.read(name)).hexdigest() == sha
        artifact = {'target': target, 'sha256': digest(archive), 'executable': executable, 'executableSha256': files[executable]}
        (output / f'{slug}-{target}.artifact.json').write_text(json.dumps(artifact, indent=2) + '\n')
        (output / f'{archive.name}.sha256').write_text(f"{artifact['sha256']}  {archive.name}\n")
        (output / f'{slug}-{target}.executable.sha256').write_text(f"{artifact['executableSha256']}  {executable}\n")
        (output / f'{slug}-{target}.files.json').write_text(json.dumps(files, indent=2) + '\n')
        print(archive)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--target', required=True, choices=TARGETS)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    build(args.target, args.binary, args.output)
