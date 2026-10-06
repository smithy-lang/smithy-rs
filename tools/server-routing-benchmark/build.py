#!/usr/bin/env python3
"""Build benchmark executables from retained generated servers (never runs codegen)."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from prepare import HERE, PROTOCOLS

def write_if_changed(path, contents):
    if not path.exists() or path.read_text() != contents:
        path.write_text(contents)

OPS = ['Echo', 'Ping', 'Read', 'Write', 'List', 'Find', 'Update', 'Delete']
def build(cache, side):
    dest = cache / side
    stamp = dest / 'generation.json'
    if not stamp.exists():
        raise SystemExit(f'Run prepare.py first: {stamp} is absent')
    repo = Path(json.loads(stamp.read_text())['repo'])
    variants = list(PROTOCOLS) if side == 'baseline' else ['multi']
    runner = dest / 'runner'
    (runner / 'src').mkdir(parents=True, exist_ok=True)
    deps = '\n'.join(f'bench_{v} = {{ path = {json.dumps(str(dest / "generated" / v / "rust-server-codegen"))} }}' for v in variants)
    tower = '0.4' if side == 'baseline' else '0.5'
    write_if_changed(runner / 'Cargo.toml', f'''[package]
name = "routing-benchmark-{side}"
version = "0.0.0"
edition = "2021"
[workspace]
[features]
allocations = []
[dependencies]
{deps}
bytes = "1"
http = "1"
http-body = "1"
http-body-util = "0.1"
tower = {{ version = "{tower}", features = ["util"] }}
tokio = {{ version = "1", features = ["rt", "time", "net"] }}
serde_json = "1"
ciborium = "0.2"
libc = "0.2"
[profile.release]
debug = 1
''')
    arms = []
    for variant in variants:
        sdk = f'bench_{variant}'
        handlers = '\n'.join(f'.{op.lower()}(|input: {sdk}::input::{op}Input| async move {{ {sdk}::output::{op}Output::builder().value(black_box(input.value.expect("benchmark request contains value"))).build().unwrap() }})' for op in OPS)
        pattern = '_' if variant == 'multi' else json.dumps(variant)
        arms.append(f'''{pattern} => {{
            let app = {sdk}::BenchmarkService::builder({sdk}::BenchmarkServiceConfig::builder().build())
                {handlers}.build().unwrap();
            run(app, &protocol, size);
        }}''')
    if side == 'baseline': arms.append('_ => panic!("unknown protocol")')
    main = '''fn main() {
    let mut args = std::env::args().skip(1);
    let protocol = args.next().expect("protocol");
    let size: usize = args.next().expect("value size").parse().unwrap();
    match protocol.as_str() {
''' + ',\n'.join(arms) + '\n    }\n}\n'
    write_if_changed(runner / 'src/main.rs', (HERE / 'harness.rs').read_text().replace('// SERVER_MAIN', main))
    from run import fingerprint
    source_hash = fingerprint(repo)
    for feature, target in [([], 'target-cpu'), (['--features', 'allocations'], 'target-memory')]:
        subprocess.run(['cargo', 'build', '--quiet', '--release', '--manifest-path', str(runner / 'Cargo.toml'), '--target-dir', str(dest / target), *feature], cwd=repo, check=True)
    if fingerprint(repo) != source_hash:
        raise SystemExit('Runtime sources changed during compilation; rerun build.py')
    (dest / 'build.json').write_text(json.dumps({
        'runtime_source_sha256': source_hash,
        'harness_sha256': hashlib.sha256((HERE / 'harness.rs').read_bytes()).hexdigest(),
    }, indent=2))

if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--cache', type=Path, default=HERE / 'artifacts')
    args = p.parse_args()
    for side in ['baseline', 'current']: build(args.cache.resolve(), side)
