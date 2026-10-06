#!/usr/bin/env python3
"""Generate servers once; subsequent invocations reuse the retained sources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
PROTOCOLS = {
    'rest_json': '@aws.protocols#restJson1',
    'rest_xml': '@aws.protocols#restXml',
    'aws_json_10': '@aws.protocols#awsJson1_0',
    'aws_json_11': '@aws.protocols#awsJson1_1',
    'rpc_v2_cbor': '@smithy.protocols#rpcv2Cbor',
}

def prepare(root, side, repo):
    dest = root / side
    stamp = dest / 'generation.json'
    template = (HERE / 'model.smithy').read_text()
    if stamp.exists():
        data = json.loads(stamp.read_text())
        if data['repo'] != str(repo):
            raise SystemExit(f'{stamp}: cached checkout differs; choose a new --cache directory')
        if data['model_sha256'] != hashlib.sha256(template.encode()).hexdigest():
            raise SystemExit(f'{stamp}: model changed; use a new --cache directory to retain the old servers')
        variants = PROTOCOLS if side == 'baseline' else {'multi': ''}
        for name in variants:
            if not (dest / 'generated' / name / 'rust-server-codegen' / 'Cargo.toml').exists():
                raise SystemExit(f'{stamp}: incomplete cache; missing crate {name}')
        print(f'Reusing generated {side} servers in {dest}', flush=True)
        return
    dest.mkdir(parents=True, exist_ok=True)
    variants = PROTOCOLS if side == 'baseline' else {'multi': '\n'.join(PROTOCOLS.values())}
    projections = {}
    for name, traits in variants.items():
        model = dest / f'{name}.smithy'
        model.write_text(template.replace('// PROTOCOLS', traits))
        projections[name] = {
            'imports': [str(model)],
            'plugins': {'rust-server-codegen': {
                'service': 'benchmark.routing#BenchmarkService',
                'module': f'bench_{name}', 'moduleVersion': '0.0.1',
                'moduleAuthors': ['Benchmark'], 'moduleDescription': 'Retained routing benchmark server',
                'runtimeConfig': {'relativePath': str(repo / 'rust-runtime')},
                'codegen': {'http-1x': True, **({'schemaSerde': True} if side == 'current' else {})},
            }},
        }
    config = dest / 'smithy-build.json'
    config.write_text(json.dumps({'version': '1.0', 'projections': projections}, indent=2))
    env = {**os.environ, 'ROUTING_BENCH_CONFIG': str(config), 'ROUTING_BENCH_OUTPUT': str(dest / 'generated')}
    subprocess.run([str(repo / 'gradlew'), '-I', str(HERE / 'codegen.init.gradle'), ':codegen-server:generateRoutingBenchmark', '--quiet'], cwd=repo, env=env, check=True)
    for name in variants:
        if not (dest / 'generated' / name / 'rust-server-codegen' / 'Cargo.toml').exists():
            raise SystemExit(f'Missing generated crate: {name}')
    diff = subprocess.check_output(['git', 'diff', 'HEAD', '--binary'], cwd=repo)
    (dest / 'generation.diff').write_bytes(diff)
    stamp.write_text(json.dumps({'repo': str(repo), 'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(), 'model_sha256': hashlib.sha256(template.encode()).hexdigest(), 'diff_sha256': hashlib.sha256(diff).hexdigest()}, indent=2))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, default=HERE / 'artifacts')
    parser.add_argument('--baseline', type=Path, default=Path.home() / 'smithy-rs-latest')
    parser.add_argument('--current', type=Path, default=HERE.parents[1])
    args = parser.parse_args()
    for side, repo in [('baseline', args.baseline), ('current', args.current)]:
        prepare(args.cache.resolve(), side, repo.resolve())
