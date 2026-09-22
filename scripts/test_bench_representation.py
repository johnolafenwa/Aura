import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('representation_bench', Path(__file__).with_name('bench-representation.py'))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class RepresentationBenchmarkTests(unittest.TestCase):
    def test_parse_stats_reads_every_counter_for_the_requested_backend(self):
        stderr = ('aura runtime stats (mir): union_payload_boxes=2 closure_environments=1 '
                  'opaque_boxes=0 callable_overflow_allocations=0\n'
                  'aura runtime stats (direct): union_payload_boxes=1 closure_environments=1 '
                  'opaque_boxes=10 callable_overflow_allocations=0\n')
        self.assertEqual(bench.parse_stats(stderr, 'direct')['opaque_boxes'], 10)
        self.assertEqual(bench.parse_stats(stderr, 'mir')['union_payload_boxes'], 2)

    def test_parse_stats_preserves_payload_clone_observations(self):
        stderr = ('aura runtime stats (direct): union_payload_boxes=1 closure_environments=2 '
                  'opaque_boxes=3 callable_overflow_allocations=4 '
                  'payload_container_clone_observations=5 '
                  'payload_fallible_clone_observations=6 '
                  'payload_value_clone_observations=7\n')
        self.assertEqual(bench.parse_stats(stderr, 'direct'), {
            'union_payload_boxes': 1,
            'closure_environments': 2,
            'opaque_boxes': 3,
            'callable_overflow_allocations': 4,
            'payload_container_clone_observations': 5,
            'payload_fallible_clone_observations': 6,
            'payload_value_clone_observations': 7,
        })

    def test_parse_stats_keeps_legacy_reports_without_inventing_clone_counts(self):
        stderr = ('aura runtime stats (mir): union_payload_boxes=2 closure_environments=1 '
                  'opaque_boxes=0 callable_overflow_allocations=0\n')
        self.assertEqual(bench.parse_stats(stderr, 'mir'), {
            'union_payload_boxes': 2,
            'closure_environments': 1,
            'opaque_boxes': 0,
            'callable_overflow_allocations': 0,
        })

    def test_parse_stats_rejects_missing_or_unknown_counters(self):
        with self.assertRaisesRegex(ValueError, 'missing counters'):
            bench.parse_stats('aura runtime stats (mir): union_payload_boxes=2\n', 'mir')
        with self.assertRaisesRegex(ValueError, 'unexpected counter'):
            bench.parse_stats('aura runtime stats (mir): heap=2\n', 'mir')
        with self.assertRaisesRegex(ValueError, 'no direct stats line'):
            bench.parse_stats('', 'direct')

    def test_summary_pairs_backends_and_requires_identical_output(self):
        stats = {name: 0 for name in bench.COUNTERS}
        records = {'union_scalar_local': {
            'mir': {'stdout': '1\n', 'elapsed_s': 0.5, 'stats': stats},
            'direct': {'stdout': '1\n', 'elapsed_s': 0.1, 'stats': stats, 'binary_bytes': 12}}}
        summary = bench.summarize(records)
        self.assertEqual(summary['union_scalar_local']['direct']['binary_bytes'], 12)
        records['union_scalar_local']['direct']['stdout'] = '2\n'
        with self.assertRaisesRegex(ValueError, 'disagree'):
            bench.summarize(records)


if __name__ == '__main__':
    unittest.main()
