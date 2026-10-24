import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    'aura_libraries', Path(__file__).with_name('check-aura-libraries.py'))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

READELF_OUTPUT = """\

Dynamic section at offset 0x4180c38 contains 31 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libz.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libzstd.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libstdc++.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [ld-linux-x86-64.so.2]
 0x000000000000000c (INIT)               0x1000
"""

OTOOL_OUTPUT = """\
target/release/aura:
\t/usr/lib/libz.1.dylib (compatibility version 1.0.0, current version 1.2.12)
\t/opt/homebrew/opt/zstd/lib/libzstd.1.dylib (compatibility version 1.0.0, current version 1.5.7)
\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 1800.105.0)
\t/System/Library/Frameworks/Security.framework/Versions/A/Security (compatibility version 1.0.0, current version 61123.0.0)
\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)
"""


class AuraLibraryCheckTests(unittest.TestCase):
    def test_linux_reads_the_needed_entries(self):
        self.assertEqual(checker.linux_libraries(READELF_OUTPUT), [
            'libm.so.6', 'libz.so.1', 'libzstd.so.1', 'libstdc++.so.6',
            'libgcc_s.so.1', 'libc.so.6', 'ld-linux-x86-64.so.2',
        ])

    def test_linux_allows_glibc_the_cxx_runtime_zlib_and_zstd(self):
        libraries = checker.linux_libraries(READELF_OUTPUT)
        self.assertEqual(checker.disallowed('Linux', libraries), [])

    def test_linux_rejects_a_library_users_may_not_have(self):
        self.assertEqual(
            checker.disallowed('Linux', ['libc.so.6', 'libxml2.so.2', 'libLLVM.so.22.1']),
            ['libxml2.so.2', 'libLLVM.so.22.1'])

    def test_macos_reads_the_load_commands_without_the_file_line(self):
        self.assertEqual(checker.macos_libraries(OTOOL_OUTPUT), [
            '/usr/lib/libz.1.dylib',
            '/opt/homebrew/opt/zstd/lib/libzstd.1.dylib',
            '/usr/lib/libc++.1.dylib',
            '/System/Library/Frameworks/Security.framework/Versions/A/Security',
            '/usr/lib/libSystem.B.dylib',
        ])

    def test_macos_rejects_homebrew_libraries(self):
        libraries = checker.macos_libraries(OTOOL_OUTPUT)
        self.assertEqual(checker.disallowed('Darwin', libraries),
                         ['/opt/homebrew/opt/zstd/lib/libzstd.1.dylib'])

    def test_other_systems_have_no_rule(self):
        with self.assertRaises(ValueError):
            checker.disallowed('Windows', [])


if __name__ == '__main__':
    unittest.main()
