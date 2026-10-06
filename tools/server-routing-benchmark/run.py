#!/usr/bin/env python3
"""Compare retained benchmark binaries; never generates servers or builds code."""
import argparse
import csv
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
from prepare import HERE, PROTOCOLS


def fingerprint(repo):
    files = sorted((repo / 'rust-runtime').rglob('*.rs'))
    files += sorted((repo / 'rust-runtime').glob('*/Cargo.toml'))
    digest = hashlib.sha256()
    for path in files:
        if 'target' not in path.parts:
            digest.update(str(path.relative_to(repo)).encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, default=HERE / 'artifacts')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--cpu', type=int, help='Default: lowest CPU in the allowed affinity set')
    parser.add_argument('--protocols', nargs='+', choices=list(PROTOCOLS), default=list(PROTOCOLS))
    parser.add_argument('--sizes', type=int, nargs='+', default=[64, 1024, 16384])
    args = parser.parse_args()
    if args.repeats < 1: parser.error('--repeats must be positive')
    cpu = args.cpu if args.cpu is not None else min(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpu})
    cache = args.cache.resolve()
    output = args.output or cache / 'results' / datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    output.mkdir(parents=True, exist_ok=False)
    metadata = {'cpu_affinity': cpu, 'platform': platform.platform(), 'repeats': args.repeats,
                'sizes': args.sizes, 'protocols': args.protocols, 'cpuinfo': Path('/proc/cpuinfo').read_text(), 'loadavg': Path('/proc/loadavg').read_text(),
                'method': 'In-process sequential full pipeline; one service clone and oneshot per request; Full<Bytes> input; eight operations; CPU uninstrumented; memory separately instrumented; no sockets or TLS', 'sides': {}}
    for side in ['baseline', 'current']:
        dest = cache / side
        generation = json.loads((dest / 'generation.json').read_text())
        repo = Path(generation['repo'])
        built = dest / 'build.json'
        runtime_hash = fingerprint(repo)
        if built.exists():
            build_info = json.loads(built.read_text())
            if build_info['runtime_source_sha256'] != runtime_hash or build_info['harness_sha256'] != hashlib.sha256((HERE / 'harness.rs').read_bytes()).hexdigest():
                raise SystemExit(f'{side} binaries are stale; run build.py')
        else:
            raise SystemExit(f'Missing {built}; run build.py')
        metadata['sides'][side] = {'generation': generation,
            'revision_at_run': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
            'runtime_source_sha256': runtime_hash,
            'status': subprocess.check_output(['git', 'status', '--short'], cwd=repo, text=True),
            'rustc': subprocess.check_output(['rustc', '-Vv'], cwd=repo, text=True)}
        (output / f'{side}.diff').write_bytes(subprocess.check_output(['git', 'diff', 'HEAD', '--binary'], cwd=repo))
        for kind in ['cpu', 'memory']:
            binary = dest / f'target-{kind}' / 'release' / f'routing-benchmark-{side}'
            if not binary.exists(): raise SystemExit(f'Missing binary {binary}; run build.py')
            metadata['sides'][side][f'{kind}_binary_sha256'] = hashlib.sha256(binary.read_bytes()).hexdigest()
        metadata['sides'][side]['lock_sha256'] = hashlib.sha256((dest / 'runner/Cargo.lock').read_bytes()).hexdigest()
    (output / 'metadata.json').write_text(json.dumps(metadata, indent=2))
    rows = []
    raw = output / 'samples.jsonl'
    for protocol in args.protocols:
        for size in args.sizes:
            for repeat in range(args.repeats):
                # Alternate which implementation goes first to reduce time/order bias.
                for side in (['baseline', 'current'] if repeat % 2 == 0 else ['current', 'baseline']):
                    for kind in (['cpu', 'memory'] if repeat == 0 else ['cpu']):
                        dest = cache / side
                        binary = dest / f'target-{kind}' / 'release' / f'routing-benchmark-{side}'
                        rss = output / 'process-memory.txt'
                        result = subprocess.run(['/usr/bin/time', '-f', '%M', '-o', str(rss), str(binary), protocol, str(size)], text=True, capture_output=True, check=True)
                        row = json.loads(result.stdout)
                        row.update(side=side, kind=kind, repeat=repeat, peak_rss_kib=int(rss.read_text().strip()))
                        rows.append(row)
                        with raw.open('a') as f: f.write(json.dumps(row) + '\n')
                        print(f'{protocol} {size}B {side} {kind} repeat={repeat + 1}', flush=True)
    summaries = []
    for protocol in args.protocols:
        for size in args.sizes:
            record = {'protocol': protocol, 'value_bytes': size}
            for side in ['baseline', 'current']:
                group = [r for r in rows if r['protocol'] == protocol and r['value_bytes'] == size and r['side'] == side]
                timing = [r for r in group if r['kind'] == 'cpu']
                cpu_medians = [statistics.median(r['cpu_ns_per_request']) for r in timing]
                record[f'{side}_cpu_ns'] = statistics.median(cpu_medians)
                record[f'{side}_cpu_min_run_ns'] = min(cpu_medians)
                record[f'{side}_cpu_max_run_ns'] = max(cpu_medians)
                record[f'{side}_wall_ns'] = statistics.median([statistics.median(r['wall_ns_per_request']) for r in timing])
                record[f'{side}_peak_rss_kib'] = statistics.median(r['peak_rss_kib'] for r in timing)
                memory = next(r for r in group if r['kind'] == 'memory')
                for key in ['allocations_per_request', 'allocated_bytes_per_request', 'peak_extra_live_heap_bytes', 'net_live_heap_bytes']:
                    record[f'{side}_{key}'] = memory[key]
            record['cpu_current_over_baseline'] = record['current_cpu_ns'] / record['baseline_cpu_ns']
            record['allocated_bytes_current_over_baseline'] = record['current_allocated_bytes_per_request'] / record['baseline_allocated_bytes_per_request']
            summaries.append(record)
    with (output / 'summary.csv').open('w') as f:
        writer = csv.DictWriter(f, fieldnames=summaries[0].keys())
        writer.writeheader()
        writer.writerows(summaries)
    (output / 'summary.json').write_text(json.dumps(summaries, indent=2))
    print(f'Results: {output}')

if __name__ == '__main__': main()
