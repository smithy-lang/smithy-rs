#!/usr/bin/env python3
"""Fresh generated legacy/schema servers exercised by the live HTTP/2 event-stream client."""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SUITE = Path(__file__).resolve().parent

def main():
    run = SUITE / '.runs' / ('streaming-input-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ'))
    run.mkdir(parents=True)
    print(f'Run: {run}', flush=True)
    env = dict(os.environ, STREAMING_INPUT_ROOT=str(ROOT), STREAMING_INPUT_RUN=str(run))
    env.setdefault('CARGO_TARGET_DIR', str(SUITE / '.runs/streaming-input-target'))
    with (run / 'generation.log').open('w') as log:
        subprocess.run([str(ROOT/'gradlew'), ':codegen-server:test', '--rerun', '--tests', '*StreamingInputLiveGenerationTest', '--quiet'], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    results = {}
    for mode in ['legacy', 'schema']:
        probe = run / (mode+'-probe')
        source = ROOT/'codegen-server-test/integration-tests/eventstreams'
        shutil.copytree(source/'src', probe/'src')
        shutil.copytree(source/'tests', probe/'tests')
        client_path = probe/'src/lib.rs'
        client = client_path.read_text().replace('let mut body = response.into_body();', 'println!("WIRE status={} headers={:?}", response.status(), response.headers());\n            let mut body = response.into_body();')
        client = client.replace('if let Some(data) = frame.data_ref() {', 'if let Some(data) = frame.data_ref() {\n                            println!("WIRE body={:02x?}", data.as_ref());')
        client_path.write_text(client)
        manifest = (source/'Cargo.toml').read_text()
        manifest += '\n[workspace]\n'
        for dep in ['rpcv2cbor_extras', 'rpcv2cbor_extras_no_initial_response']:
            sdk = run/(mode+('-no-initial' if dep.endswith('no_initial_response') else '-initial'))
            # Integration tests append their HTTP-version suffix to the selected directory.
            candidates = [sdk, *run.glob(sdk.name+'*')]
            sdk = next(p for p in candidates if (p/'Cargo.toml').is_file())
            package = tomllib.loads((sdk/'Cargo.toml').read_text())['package']['name']
            manifest = manifest.replace(dep+' = { workspace = true }', dep+' = { path = '+json.dumps(str(sdk))+', package = '+json.dumps(package)+' }')
        for dep in ['aws-smithy-eventstream','aws-smithy-http','aws-smithy-cbor','aws-smithy-runtime']:
            features = ', features = ["test-util"]' if dep in ['aws-smithy-eventstream','aws-smithy-runtime'] else ''
            manifest = manifest.replace(dep+' = { workspace = true }', dep+' = { path = '+json.dumps(str(ROOT/'rust-runtime'/dep))+features+' }')
        manifest = manifest.replace('path = "../../../rust-runtime/aws-smithy-types"', 'path = '+json.dumps(str(ROOT/'rust-runtime/aws-smithy-types')))
        (probe/'Cargo.toml').write_text(manifest)
        with (run/(mode+'.log')).open('w') as log:
            result = subprocess.run(['cargo','test','--quiet','--test','structured_eventstream_tests','--','--nocapture'],cwd=probe,env=env,stdout=log,stderr=subprocess.STDOUT)
        results[mode] = result.returncode
        print(f'{mode}: exit {result.returncode}; {run/(mode+".log")}',flush=True)
    (run/'results.json').write_text(json.dumps(results,indent=2)+'\n')
    return int(any(results.values()))

if __name__ == '__main__':
    raise SystemExit(main())
