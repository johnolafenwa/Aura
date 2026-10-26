"""Manifest, build commands, and result checks for the cross-language programs."""
import importlib.util
from pathlib import Path
import re
import unittest

spec = importlib.util.spec_from_file_location('cross_language_bench', Path(__file__).with_name('bench-cross-language.py'))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

RUST_BIN = re.compile(r'\[\[bin\]\]\nname = "(\w+)"\npath = "\.\./cross_language/(\w+)/(\w+)\.rs"\n'
                      r'required-features = \["cross-language"\]\n')


class CrossLanguageBenchmarkTests(unittest.TestCase):
    def test_every_program_directory_is_in_the_manifest_with_three_versions(self):
        programs = bench.load_programs()
        directories = {path.name for path in bench.BENCH_DIR.iterdir() if path.is_dir()}
        self.assertEqual(directories, set(programs))
        for name in programs:
            for suffix in ('au', 'py', 'rs'):
                self.assertTrue((bench.BENCH_DIR / name / f'{name}.{suffix}').is_file(), (name, suffix))

    def test_every_program_is_a_rust_bin_behind_the_cross_language_feature(self):
        manifest = (bench.rust_baselines.PROJECT / 'Cargo.toml').read_text()
        bins = RUST_BIN.findall(manifest)
        self.assertEqual({name for name, _, _ in bins}, set(bench.load_programs()))
        for name, directory, file in bins:
            self.assertEqual((directory, file), (name, name))

    def test_expected_lines_name_the_program_and_both_sizes_take_the_same_arguments(self):
        for name, program in bench.load_programs().items():
            self.assertTrue(program['expected'].startswith(name + ' '), name)
            self.assertNotIn('\n', program['expected'])
            self.assertEqual(len(program['check']), len(program['bench']), name)
            self.assertEqual(program['check'].count(bench.WORKDIR), program['bench'].count(bench.WORKDIR))

    def test_workdir_placeholder_is_replaced(self):
        self.assertEqual(bench.expand_args(['10', '{workdir}'], '/tmp/run'), ['10', '/tmp/run'])
        self.assertEqual(bench.expand_args(['10'], '/tmp/run'), ['10'])

    def test_rust_build_is_locked_pinned_and_feature_gated(self):
        command = bench.rust_build_command('/target', 'cross-language', ['nbody', 'lru_cache'])
        self.assertEqual(command[:3], ['cargo', '+1.95.0', 'build'])
        self.assertIn('--locked', command)
        self.assertEqual(command[command.index('--profile') + 1], 'cross-language')
        self.assertEqual(command[command.index('--features') + 1], 'cross-language')
        self.assertEqual(command[-4:], ['--bin', 'nbody', '--bin', 'lru_cache'])

    def test_summary_requires_matching_output_and_reports_ratios(self):
        record = {'stdout': {'python': 'x 1\n', 'rust': 'x 1\n', 'aura': 'x 1\n'},
                  'samples_s': {'python': [0.2, 0.1, 0.3], 'rust': [0.01], 'aura': [0.4]}}
        row = bench.summarize_record('x', record)
        self.assertAlmostEqual(row['python_ms'], 200.0)
        self.assertAlmostEqual(row['aura_vs_python'], 2.0)
        self.assertAlmostEqual(row['aura_vs_rust'], 40.0)
        self.assertNotIn('aura_instructions', row)
        record['stdout']['aura'] = 'x 2\n'
        with self.assertRaisesRegex(bench.BenchmarkError, 'aura printed'):
            bench.summarize_record('x', record)

    def test_callgrind_totals_are_parsed_and_shown_as_a_ratio(self):
        stderr = '==12== Events    : Ir\n==12== Collected : 123456\n==12==\n==12== I   refs:      123,456\n'
        self.assertEqual(bench.callgrind_instructions(stderr), 123456)
        with self.assertRaises(bench.BenchmarkError):
            bench.callgrind_instructions('')
        record = {'stdout': {'python': 'x\n', 'rust': 'x\n', 'aura': 'x\n'},
                  'samples_s': {'python': [0.1], 'rust': [0.01], 'aura': [0.05]},
                  'instructions': {'rust': 2_000_000, 'aura': 20_000_000}}
        row = bench.summarize_record('x', record)
        self.assertAlmostEqual(row['aura_vs_rust_instructions'], 10.0)
        self.assertIn('(10.0x)', bench.format_row('x', row))


if __name__ == '__main__':
    unittest.main()
