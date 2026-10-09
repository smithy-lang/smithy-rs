# Compile the real harness allocator without generating server SDKs.
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


HERE = Path(__file__).resolve().parent


def allocator_source():
    source = (HERE / "harness.rs").read_text()
    start = source.index('#[cfg(feature = "allocations")]\nmod allocations {')
    end = source.index('#[cfg(feature = "allocations")]\n#[global_allocator]', start)
    return source[start:end]


class AllocatorTests(unittest.TestCase):
    def test_allocation_sequences(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            source = directory / "tests.rs"
            source.write_text(allocator_source() + (HERE / "allocator_tests.rs").read_text())
            binary = directory / "tests"
            subprocess.run([
                "rustc", "--edition", "2021", "--cfg", 'feature="allocations"',
                "--test", str(source), "-o", str(binary),
            ], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)

    def test_process_global_allocator_and_output_snapshot(self):
        probe = r'''
#[global_allocator]
static GLOBAL: allocations::Counting = allocations::Counting;
fn main() {
    use std::alloc::{alloc, dealloc, Layout};
    unsafe {
        let warmup_layout = Layout::from_size_align(128, 8).unwrap();
        let measured_layout = Layout::from_size_align(64, 8).unwrap();
        let warmup = alloc(warmup_layout);
        assert!(!warmup.is_null());
        let baseline = allocations::begin_measurement();
        dealloc(warmup, warmup_layout);
        let measured = alloc(measured_layout);
        assert!(!measured.is_null());
        let result = allocations::finish_measurement(baseline);
        let output_work = alloc(warmup_layout);
        assert!(!output_work.is_null());
        assert!(baseline >= 128);
        assert_eq!(result.allocation_calls, 1);
        assert_eq!(result.allocated_bytes, 64);
        println!("{{\"baseline\":{},\"peak\":{},\"final\":{},\"delta\":{},\"growth\":{}}}",
            result.baseline_live_heap_bytes, result.peak_live_heap_bytes,
            result.final_live_heap_bytes, result.net_heap_delta_bytes,
            result.peak_heap_growth_bytes);
        dealloc(measured, measured_layout);
        dealloc(output_work, warmup_layout);
    }
}
'''
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            source = directory / "probe.rs"
            source.write_text(allocator_source() + probe)
            binary = directory / "probe"
            subprocess.run([
                "rustc", "--edition", "2021", "--cfg", 'feature="allocations"',
                str(source), "-o", str(binary),
            ], check=True)
            result = subprocess.run([str(binary)], check=True, capture_output=True, text=True)
        values = json.loads(result.stdout)
        self.assertGreaterEqual(values["baseline"], 128)
        self.assertEqual(values["peak"], values["baseline"])
        self.assertEqual(values["final"], values["baseline"] - 64)
        self.assertEqual(values["delta"], -64)
        self.assertEqual(values["growth"], 0)


if __name__ == "__main__":
    unittest.main()
