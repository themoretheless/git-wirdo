#!/usr/bin/env python3
"""Build the release probe first: cargo build --locked --release --example performance.
Then run this script with an output directory. Fixtures are disposable and seeded identically.
"""
import json, pathlib, subprocess, sys, tempfile, platform, hashlib, os, time
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
binary = pathlib.Path(sys.argv[3]).resolve() if len(sys.argv)>3 else pathlib.Path(__file__).resolve().parents[1] / 'target/release/examples/performance'
runs=[]
if len(sys.argv)>2 and hashlib.sha256(binary.read_bytes()).digest()==hashlib.sha256(pathlib.Path(sys.argv[2]).read_bytes()).digest():
    raise SystemExit('Before/after binaries are identical: rebuild each revision in separate target directories.')
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
            stream.extend(f'M 100644 inline base\ndata {len(body)}\n'.encode()+body+b'\n')
            if n == 0:
                detail=''.join(f'base line {i} — текст\n' for i in range(30000)).encode()
                stream.extend(f'M 100644 inline zz-large-detail\ndata {len(detail)}\n'.encode()+detail+b'\n')
            stream.extend(b'\n')
        git(root, 'fast-import', '--quiet', data=stream)
        git(root, 'reset', '--hard', 'HEAD')
        for n in range(files): (root/f'file-{n:05}').write_text('changed\n'*100)
        (root/'zz-large-detail').write_text(''.join(f'base line {n} — текст\n' for n in range(30000)))
        git(root, 'add', '--all')
        for n in range(files): (root/f'file-{n:05}').write_text('pending\n'*100)
        (root/'zz-large-detail').write_text(''.join(f'pending line {n} — текст\n' for n in range(30000)))
        probes = [('after', binary)]
        if len(sys.argv)>2: probes.insert(0, ('before', pathlib.Path(sys.argv[2]).resolve()))
        if name=='history': probes.reverse()
        for label, executable in probes:
            command=[str(executable), str(root)]
            if sys.platform=='darwin': command=['/usr/bin/time','-l',*command]
            started=time.monotonic()
            load_before=os.getloadavg() if hasattr(os, 'getloadavg') else None
            result = subprocess.run(command, text=True, capture_output=True)
            runs.append({'case':name,'variant':label,'binary_sha256':hashlib.sha256(executable.read_bytes()).hexdigest(),'elapsed_seconds':time.monotonic()-started,'load_before':load_before,'load_after':os.getloadavg() if hasattr(os, 'getloadavg') else None})
            (out/f'{name}-{label}.jsonl').write_text(result.stdout)
            (out/f'{name}-{label}-resources.txt').write_text(result.stderr)
            print(name, label, result.stdout, flush=True)
            result.check_returncode()
    (out/'environment.json').write_text(json.dumps({'runs':runs, 'platform': platform.platform(), 'rustc': subprocess.check_output(['rustc','--version'],text=True).strip()}, indent=2))
