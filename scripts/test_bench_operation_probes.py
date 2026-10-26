"""Manifest, sources, and result math for the operation probes."""
import importlib.util
from pathlib import Path
import re
import unittest

spec = importlib.util.spec_from_file_location('operation_probes_bench', Path(__file__).with_name('bench-operation-probes.py'))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)

RUST_BIN = ('[[bin]]\nname = "operation_probes"\npath = "../operation_probes/operation_probes.rs"\n'
            'required-features = ["cross-language"]\n')


class OperationProbeTests(unittest.TestCase):
    def test_both_versions_dispatch_exactly_the_manifest_operations(self):
        operations = set(bench.load_operations())
        aura = (bench.PROBE_DIR / 'operation_probes.au').read_text()
        rust = (bench.PROBE_DIR / 'operation_probes.rs').read_text()
        self.assertEqual(set(re.findall(r'operation == "(\w+)"', aura)), operations)
        self.assertEqual(set(re.findall(r'^\s+"(\w+)" =>', rust, re.MULTILINE)), operations)

    def test_every_operation_names_a_count_a_plan_step_and_what_it_measures(self):
        for name, operation in bench.load_operations().items():
            self.assertEqual(set(operation), {'count', 'plan_step', 'measures'}, name)
            self.assertIsInstance(operation['count'], int)
            self.assertGreater(operation['count'], bench.CHECK_COUNT, name)

    def test_the_rust_version_is_a_bin_behind_the_cross_language_feature(self):
        manifest = (bench.cross.rust_baselines.PROJECT / 'Cargo.toml').read_text()
        self.assertIn(RUST_BIN, manifest)

    def test_cost_is_the_run_minus_the_setup_per_operation(self):
        self.assertEqual(bench.per_operation(1_000, 51_000, 100), 500.0)

    def test_outputs_must_match_in_both_phases_and_name_the_operation(self):
        outputs = {('rust', 'setup'): 'op 0', ('aura', 'setup'): 'op 0',
                   ('rust', 'run'): 'op 7', ('aura', 'run'): 'op 7'}
        bench.same_output('op', outputs)
        with self.assertRaisesRegex(bench.cross.BenchmarkError, 'Aura printed'):
            bench.same_output('op', {**outputs, ('aura', 'run'): 'op 8'})
        with self.assertRaisesRegex(bench.cross.BenchmarkError, 'unexpected output'):
            bench.same_output('other', outputs)

    def test_rows_show_a_ratio_only_when_rust_does_measurable_work(self):
        operation = {'plan_step': '1a'}
        row = {'count': 1000, 'rust': 2.0, 'aura': 20.0}
        self.assertTrue(bench.format_row('op', operation, row).endswith('10.0x'))
        row['rust'] = 0.1
        self.assertTrue(bench.format_row('op', operation, row).endswith(' -'))


if __name__ == '__main__':
    unittest.main()
