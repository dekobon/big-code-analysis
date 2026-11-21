#!/usr/bin/env python3
"""Exercise maintained fuzz argv and environment with deterministic fake commands."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class FuzzMakeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="bca-fuzz-make-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.record = self.root / "calls.jsonl"
        self.work = self.root / "writable corpus"
        self.corpus = self.root / "read only seeds"
        for target in ["documentation", "parse_rust"]:
            path = self.corpus / target
            path.mkdir(parents=True)
            (path / "seed").write_bytes(b"seed\x00bytes")
        self.before = self.inventory()
        self.env = os.environ | {"PATH": str(self.bin), "RECORD": str(self.record)}
        for name in ["grep", "sed", "mkdir", "cut", "bash"]:
            os.symlink(shutil.which(name), self.bin / name)
        self.command("rustup", "print('nightly-aarch64-unknown-linux-gnu')")
        self.command("rustc", "print('host: aarch64-unknown-linux-gnu')")
        self.command("cargo-fuzz", "pass")
        self.command("cargo", """
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
if args == ['+nightly', 'fuzz', 'list']:
    print('documentation\\nparse_rust')
else:
    record = {'argv': args, 'env': {k: v for k, v in os.environ.items() if k.startswith(('CFLAGS_', 'CXXFLAGS_', 'LSAN_', 'ASAN_'))}}
    with open(os.environ['RECORD'], 'a') as f:
        f.write(json.dumps(record) + '\\n')
    corpus = args[args.index('--target') + 3:args.index('--')]
    if len(corpus) == 2:
        assert Path(corpus[0]).is_dir()
        assert Path(corpus[1]).is_dir()
        (Path(corpus[0]) / 'discovery').write_bytes(b'discovery')
""")

    def command(self, name, body):
        path = self.bin / name
        path.write_text(f"#!{shutil.which('python3')}\n{body}\n")
        path.chmod(0o755)

    def inventory(self):
        return {str(p.relative_to(self.corpus)): p.read_bytes() for p in self.corpus.rglob('*') if p.is_file()}

    def run_make(self, target, *settings, symbolizer="/fake/llvm symbolizer"):
        command = [shutil.which("make"), "--no-print-directory", "-f", str(ROOT / "Makefile"), target,
                   f"FUZZ_WORK={self.work}", f"FUZZ_CORPUS={self.corpus}", f"FUZZ_SYMBOLIZER={symbolizer}", *settings]
        result = subprocess.run(command, cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(self.inventory(), self.before, result.stdout + result.stderr)
        return result

    def records(self):
        return [json.loads(line) for line in self.record.read_text().splitlines()] if self.record.exists() else []

    def check_call(self, record, target, flags):
        self.assertEqual(record["argv"], ["+nightly", "fuzz", "run", "--target", "aarch64-unknown-linux-gnu", target,
                                        str(self.work / target), str(self.corpus / target), "--", *flags])
        self.assertEqual(record["env"], {
            "CFLAGS_aarch64_unknown_linux_gnu": "-fsanitize=address -fno-omit-frame-pointer",
            "CXXFLAGS_aarch64_unknown_linux_gnu": "-fsanitize=address -fno-omit-frame-pointer",
            "LSAN_OPTIONS": f"suppressions={ROOT}/fuzz/lsan-suppressions.txt",
            "ASAN_SYMBOLIZER_PATH": "/fake/llvm symbolizer",
        })

    def test_default_run_flags_and_read_only_second_corpus(self):
        result = self.run_make("fuzz-run", "FUZZ_TARGET=documentation")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(len(self.records()), 1)
        self.check_call(self.records()[0], "documentation", ["-runs=100000", "-timeout=60"])

    def test_default_corpus_paths_are_preserved_in_expanded_command(self):
        command = [shutil.which("make"), "--no-print-directory", "-n", "-f", str(ROOT / "Makefile"),
                   "fuzz-run", "FUZZ_SYMBOLIZER=/fake/symbolizer"]
        result = subprocess.run(command, cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f'"{ROOT}/fuzz/corpus-work/parse_cpp" "{ROOT}/fuzz/corpus/parse_cpp"', result.stdout)
        self.assertIn("--  -runs=100000 -timeout=60", result.stdout)
        self.assertEqual(self.records(), [])

    def test_replay_forwards_args_and_keeps_fixed_runs_last(self):
        result = self.run_make("fuzz-replay", "FUZZ_ARGS=-runs=999 -timeout=1 -print_final_stats=1 '-artifact_prefix=/a path/'", "FUZZ_TIMEOUT=73")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        records = self.records()
        self.assertEqual(len(records), 2)
        for record, target in zip(records, ["documentation", "parse_rust"]):
            self.check_call(record, target, ["-runs=999", "-timeout=1", "-print_final_stats=1", "-artifact_prefix=/a path/", "-runs=0", "-timeout=73"])

    def test_run_overrides_and_input_replay_preserve_argument_boundaries(self):
        result = self.run_make("fuzz-run", "FUZZ_TARGET=documentation", "FUZZ_RUNS=10000", "FUZZ_TIMEOUT=61", "FUZZ_ARGS=-seed=1664001 -print_final_stats=1")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.check_call(self.records()[0], "documentation", ["-seed=1664001", "-print_final_stats=1", "-runs=10000", "-timeout=61"])
        result = self.run_make("fuzz-run", "FUZZ_TARGET=parse_rust", "FUZZ_INPUT=/an input/file", "FUZZ_ARGS=-print_final_stats=1")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.records()[1]["argv"][-4:], ["/an input/file", "--", "-print_final_stats=1", "-timeout=60"])

    def test_missing_tools_are_skips_with_no_target_execution(self):
        for missing in ["nightly", "cargo-fuzz"]:
            with self.subTest(missing=missing):
                if missing == "nightly":
                    self.command("rustup", "print('stable-aarch64-unknown-linux-gnu')")
                else:
                    self.command("rustup", "print('nightly-aarch64-unknown-linux-gnu')")
                    (self.bin / "cargo-fuzz").unlink()
                result = self.run_make("fuzz-run")
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("skipping fuzz-run", result.stdout)
                self.assertEqual(self.records(), [])

    def test_absent_symbolizer_fails_before_any_target(self):
        result = self.run_make("fuzz-replay", symbolizer="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no llvm-symbolizer found", result.stdout)
        self.assertEqual(self.records(), [])


if __name__ == "__main__":
    unittest.main()
