import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    'native_binaries', Path(__file__).with_name('check-native-binaries.py'))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

COMPONENTS = {'rustls': ['rustls::'], 'serde_json': ['serde_json::'], 'ring': ['ring_core_']}
NM_OUTPUT = """\
0000000000001000 T main
0000000000001040 t rustls::client::ClientConnection::new
0000000000001100 t core::ptr::drop_in_place<serde_json::value::Value>
0000000000001110 r ring_core_0_17_14__k25519
0000000000001200 t aura_direct_print_value
not a symbol line
                 U write
"""


class NativeBinaryCheckTests(unittest.TestCase):
    def setUp(self):
        self.rows = checker.parse_nm(NM_OUTPUT)

    def test_parse_skips_undefined_and_malformed_lines(self):
        self.assertEqual([name for _, _, name in self.rows], [
            'main',
            'rustls::client::ClientConnection::new',
            'core::ptr::drop_in_place<serde_json::value::Value>',
            'ring_core_0_17_14__k25519',
            'aura_direct_print_value',
        ])

    def test_text_symbol_sizes_come_from_the_next_address(self):
        sizes = dict(checker.symbol_sizes(self.rows))
        self.assertEqual(sizes['main'], 0x40)
        self.assertEqual(sizes['rustls::client::ClientConnection::new'], 0xc0)
        self.assertEqual(sizes['core::ptr::drop_in_place<serde_json::value::Value>'], 0x10)
        self.assertNotIn('ring_core_0_17_14__k25519', sizes)
        self.assertEqual(sizes['aura_direct_print_value'], 0)

    def test_attribution_groups_symbols_and_keeps_other(self):
        totals = checker.attribute(self.rows, COMPONENTS)
        self.assertEqual(totals, {'rustls': 0xc0, 'serde_json': 0x10, 'ring': 0, 'other': 0x40})

    def test_presence_includes_data_symbols(self):
        self.assertEqual(checker.present_components(self.rows, COMPONENTS),
                         {'rustls', 'serde_json', 'ring'})

    def test_absent_component_fails(self):
        failures = checker.check_subject('hello', {'absent': ['rustls']}, self.rows, 100,
                                         COMPONENTS, None, 10)
        self.assertEqual(failures, ['hello: component `rustls` must be absent but is linked'])

    def test_missing_present_component_fails(self):
        rows = checker.parse_nm('0000000000001000 T main\n')
        failures = checker.check_subject('json', {'present': ['serde_json']}, rows, 100,
                                         COMPONENTS, None, 10)
        self.assertEqual(failures, ['json: component `serde_json` must be present but is missing'])

    def test_size_budget_allows_ten_percent_and_rejects_more(self):
        subject = {}
        self.assertEqual(checker.check_subject('hello', subject, self.rows, 1100, COMPONENTS, 1000, 10), [])
        failures = checker.check_subject('hello', subject, self.rows, 1101, COMPONENTS, 1000, 10)
        self.assertEqual(len(failures), 1)
        self.assertIn('exceeds the 10% budget', failures[0])

    def test_missing_baseline_skips_the_size_check(self):
        self.assertEqual(checker.check_subject('hello', {}, self.rows, 10**9, COMPONENTS, None, 10), [])

    def test_platform_keys_are_normalized(self):
        self.assertEqual(checker.platform_key('Linux', 'x86_64'), 'linux-x86_64')
        self.assertEqual(checker.platform_key('Darwin', 'arm64'), 'darwin-arm64')
        self.assertEqual(checker.platform_key('Linux', 'aarch64'), 'linux-arm64')

    def test_config_is_consistent(self):
        config = json.loads(checker.CONFIG.read_text())
        components = config['components']
        self.assertIn('hello_world', config['subjects'])
        for name, subject in config['subjects'].items():
            self.assertTrue((checker.ROOT / subject['source']).is_file(), name)
            listed = subject.get('absent', []) + subject.get('present', []) + subject.get('known_present', [])
            for component in listed:
                self.assertIn(component, components, f'{name}: {component}')
            self.assertFalse(set(subject.get('absent', [])) & set(subject.get('present', [])), name)
        for key, sizes in config['baselines'].items():
            for name, size in sizes.items():
                self.assertIn(name, config['subjects'], key)
                self.assertIsInstance(size, int)


if __name__ == '__main__':
    unittest.main()
