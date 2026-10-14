#!/usr/bin/env python3
"""Build the release probe first: cargo build --locked --release --example performance.
Then run this script with an output directory. Fixtures are disposable and seeded identically.
"""
import json, pathlib, subprocess, sys, tempfile, platform
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
binary = pathlib.Path(__file__).resolve().parents[1] / 'target/release/examples/performance'
def git(root, *args, data=None):
    return subprocess.run(['git', '-C', str(root), *args], input=data, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
with tempfile.TemporaryDirectory(prefix='wirdo-measure-') as directory:
    for name, commits, files in [('small', 20, 10), ('history', 3000, 10), ('files', 20, 3000)]:
        root = pathlib.Path(directory)/name; root.mkdir()
        git(root, 'init', '-q', '--initial-branch=main')
        git(root, 'config', 'core.autocrlf', 'false')
        stream = bytearray()
        for n in range(commits):
            body = f'base {n}\n'.encode()
            stream.extend(f'commit refs/heads/main\ncommitter Probe <probe@example.invalid> {1700000000+n} +0000\ndata 9\nfixture!\n'.encode())
            stream.extend(f'M 100644 inline base\ndata {len(body)}\n'.encode()+body+b'\n\n')
        git(root, 'fast-import', '--quiet', data=stream)
        git(root, 'reset', '--hard', 'HEAD')
        for n in range(files): (root/f'file-{n:05}').write_text('changed\n'*100)
        git(root, 'add', '--all')
        for n in range(files): (root/f'file-{n:05}').write_text('pending\n'*100)
        probes = [('after', binary)]
        if len(sys.argv)>2: probes.insert(0, ('before', pathlib.Path(sys.argv[2]).resolve()))
        if name=='history': probes.reverse()
        for label, executable in probes:
            command=[str(executable), str(root)]
            if sys.platform=='darwin': command=['/usr/bin/time','-l',*command]
            result = subprocess.run(command, text=True, capture_output=True)
            (out/f'{name}-{label}.jsonl').write_text(result.stdout)
            (out/f'{name}-{label}-resources.txt').write_text(result.stderr)
            print(name, label, result.stdout, flush=True)
            result.check_returncode()
    (out/'environment.json').write_text(json.dumps({'platform': platform.platform(), 'rustc': subprocess.check_output(['rustc','--version'],text=True).strip()}, indent=2))
