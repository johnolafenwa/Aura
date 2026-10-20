"""Subjects, source edits, and summaries for the compile-time measurement."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('compile_time_bench', Path(__file__).with_name('bench-compile-time.py'))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class CompileTimeBenchmarkTests(unittest.TestCase):
    def test_every_subject_has_its_entry_file(self):
        for name, (source, entry) in bench.SUBJECTS.items():
            path = bench.ROOT / source
            self.assertTrue((path / entry if path.is_dir() else path).is_file(), name)
            if not path.is_dir():
                self.assertEqual(path.name, entry)

    def test_subjects_include_the_size_gate_programs(self):
        self.assertEqual(bench.SUBJECTS['hello_world'][0], 'examples/basics/hello_world.au')
        self.assertEqual(bench.SUBJECTS['reference_agent'], ('examples/agents/tool_runner', 'src/main.au'))

    def test_each_edit_appends_a_distinct_unused_function(self):
        first = bench.edit_source('print("hi")\n', 1)
        second = bench.edit_source('print("hi")\n', 2)
        self.assertTrue(first.startswith('print("hi")\n'))
        self.assertIn('def compile_time_edit_1() -> int64:\n    return 1\n', first)
        self.assertNotEqual(first, second)

    def test_link_wrapper_calls_the_real_compiler_and_logs_its_time(self):
        wrapper = bench.link_wrapper('/usr/bin/python3', '/usr/bin/cc', Path('/tmp/link.log'))
        self.assertTrue(wrapper.startswith('#!/usr/bin/python3\n'))
        self.assertIn("subprocess.call(['/usr/bin/cc', *sys.argv[1:]])", wrapper)
        self.assertIn("open('/tmp/link.log', 'a')", wrapper)
        compile(wrapper, 'link-wrapper', 'exec')

    def test_summary_reports_median_and_spread_per_mode(self):
        summary = bench.summarize({'cold': [3.0, 1.0, 2.0], 'warm': [0.5]})
        self.assertEqual(summary['cold'], {'median_s': 2.0, 'min_s': 1.0, 'max_s': 3.0, 'samples': 3})
        self.assertEqual(summary['warm']['median_s'], 0.5)


if __name__ == '__main__':
    unittest.main()
