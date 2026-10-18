import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('runtime_values_bench', Path(__file__).with_name('bench-runtime-values.py'))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class RuntimeValuesBenchmarkTests(unittest.TestCase):
    def test_every_program_has_a_source_and_a_python_reference(self):
        reference = (bench.BENCH_DIR / 'reference.py').read_text()
        for name in bench.PROGRAMS:
            self.assertTrue((bench.BENCH_DIR / f'{name}.au').is_file(), name)
            self.assertIn(f'"{name}": {name}', reference)

    def test_parse_reference_reads_result_and_seconds(self):
        parsed = bench.parse_reference('int_list_sum 99 0.038\ndict_build_get 7 1.5\n')
        self.assertEqual(parsed['int_list_sum'], ('99', 0.038))
        self.assertEqual(parsed['dict_build_get'], ('7', 1.5))

    def test_summary_requires_matching_results_and_reports_the_ratio(self):
        records = {'int_list_sum': {'stdout': '99\n', 'elapsed_s': 0.38}}
        summary = bench.summarize(records, {'int_list_sum': ('99', 0.038)})
        self.assertAlmostEqual(summary['int_list_sum']['ratio'], 10.0)
        with self.assertRaisesRegex(ValueError, 'Python printed'):
            bench.summarize(records, {'int_list_sum': ('98', 0.038)})


if __name__ == '__main__':
    unittest.main()
