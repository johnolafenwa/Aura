#!/usr/bin/env python3
"""Behavioral tests for the ADR-0022 capability-syntax migrator."""

from __future__ import annotations

import json
import tempfile
import textwrap
import unittest
from pathlib import Path

import capability_migrate as migrate


def source(text: str) -> str:
    return textwrap.dedent(text).lstrip("\n")


class TokenAwarenessTests(unittest.TestCase):
    def test_borrow_inside_comments_and_strings_is_not_rewritten(self) -> None:
        original = source(
            '''
            # borrow self is retired, but this comment keeps saying it
            def describe(borrow self) -> String:
                note = "borrow mut String"
                other = 'borrow[label] T'
                return note
            '''
        )
        migrated = migrate.migrate_aurora(original)
        self.assertIn("# borrow self is retired", migrated)
        self.assertIn('"borrow mut String"', migrated)
        self.assertIn("'borrow[label] T'", migrated)
        self.assertIn("def describe(self) -> String:", migrated)

    def test_borrow_inside_triple_quoted_strings_is_not_rewritten(self) -> None:
        original = source(
            '''
            def doc() -> String:
                return """
            borrow self
            borrow mut T
            """
            '''
        )
        self.assertEqual(migrate.migrate_aurora(original), original)

    def test_identifiers_containing_borrow_are_not_rewritten(self) -> None:
        original = source(
            """
            def check(borrowed: String, reborrow: String) -> String:
                borrow_count = 1
                return borrowed
            """
        )
        migrated = migrate.migrate_aurora(original)
        self.assertIn("borrowed: String", migrated)
        self.assertIn("reborrow: String", migrated)
        self.assertIn("borrow_count = 1", migrated)


class ReceiverTests(unittest.TestCase):
    def test_shared_receiver_loses_the_keyword(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def read(borrow self) -> int32:\n"),
            "def read(self) -> int32:\n",
        )

    def test_mutable_receiver_becomes_mut_self(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def bump(borrow mut self):\n"),
            "def bump(mut self):\n",
        )

    def test_owning_receiver_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def close(own self):\n"),
            "def close(own self):\n",
        )

    def test_already_bare_receiver_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def read(self) -> int32:\n"),
            "def read(self) -> int32:\n",
        )


class ParameterTests(unittest.TestCase):
    def test_shared_parameter_loses_the_keyword(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def f(value: borrow String):\n"),
            "def f(value: String):\n",
        )

    def test_mutable_parameter_becomes_mut(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def f(value: borrow mut String):\n"),
            "def f(value: mut String):\n",
        )

    def test_mutable_tuple_parameter_keeps_its_type(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def f(value: borrow mut (String,)):\n"),
            "def f(value: mut (String,)):\n",
        )

    def test_labelled_parameter_drops_the_retired_label(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def f(value: borrow[src] String):\n"),
            "def f(value: String):\n",
        )

    def test_owning_parameter_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def f(value: own String):\n"),
            "def f(value: own String):\n",
        )


class ReturnAnnotationTests(unittest.TestCase):
    def test_labelled_borrowed_return_becomes_an_ordinary_return(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora(
                "def name_ref(user: borrow User) -> borrow[user] String:\n"
            ),
            "def name_ref(user: User) -> String:\n",
        )

    def test_unlabelled_borrowed_return_becomes_an_ordinary_return(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("def pick(a: borrow String) -> borrow String:\n"),
            "def pick(a: String) -> String:\n",
        )


class LoopTests(unittest.TestCase):
    def test_shared_iteration_becomes_bare(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("for value in borrow values:\n"),
            "for value in values:\n",
        )

    def test_mutable_iteration_becomes_mut(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("for value in borrow mut values:\n"),
            "for value in mut values:\n",
        )

    def test_owning_iteration_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("for value in own values:\n"),
            "for value in own values:\n",
        )

    def test_range_capability_modifiers_are_stripped(self) -> None:
        # ADR-0022's additional ruling: range yields copy values, so `mut` and
        # `own` are rejected rather than preserved as no-ops.
        self.assertEqual(
            migrate.migrate_aurora("for index in mut range(0, 3):\n"),
            "for index in range(0, 3):\n",
        )
        self.assertEqual(
            migrate.migrate_aurora("for index in own range(0, 3):\n"),
            "for index in range(0, 3):\n",
        )


class MatchTests(unittest.TestCase):
    def test_shared_match_becomes_bare(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("match borrow value:\n"),
            "match value:\n",
        )

    def test_mutable_match_becomes_match_mut(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("match borrow mut value:\n"),
            "match mut value:\n",
        )

    def test_bare_match_over_a_place_with_bindings_gains_own(self) -> None:
        original = source(
            """
            def main():
                match holder:
                    case Option.Some(value):
                        print(value)
                    case Option.None:
                        pass
            """
        )
        self.assertIn("match own holder:", migrate.migrate_aurora(original))

    def test_bare_match_over_a_temporary_stays_bare(self) -> None:
        # A call result has no surviving owner, so the consuming-to-shared flip
        # is unobservable and `own` would only add noise.
        original = source(
            """
            def main():
                match compute():
                    case Option.Some(value):
                        print(value)
                    case Option.None:
                        pass
            """
        )
        self.assertIn("match compute():", migrate.migrate_aurora(original))
        self.assertNotIn("match own", migrate.migrate_aurora(original))

    def test_bare_match_without_bindings_stays_bare(self) -> None:
        original = source(
            """
            def main():
                match holder:
                    case Option.Some(_):
                        print("some")
                    case Option.None:
                        print("none")
            """
        )
        self.assertNotIn("match own", migrate.migrate_aurora(original))

    def test_match_own_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora("match own value:\n"),
            "match own value:\n",
        )


class IdempotenceTests(unittest.TestCase):
    CORPUS = source(
        """
        class Holder:
            generator: int32

            def read(borrow self) -> int32:
                return self.generator

            def bump(borrow mut self):
                self.generator = self.generator + 1

            def close(own self) -> int32:
                return self.generator

        def name_ref(user: borrow[src] Holder) -> borrow[src] int32:
            return user.generator

        def scan(values: borrow mut Vec[String], limit: borrow int32):
            for value in borrow mut values:
                print(value)
            for index in mut range(0, limit):
                print(index)
            match borrow values:
                case _:
                    pass
            match holder:
                case Option.Some(value):
                    print(value)
                case Option.None:
                    pass
        """
    )

    def test_second_application_is_a_no_op(self) -> None:
        once = migrate.migrate_aurora(self.CORPUS)
        twice = migrate.migrate_aurora(once)
        self.assertEqual(once, twice)

    def test_migrated_source_has_no_borrow_keyword_left(self) -> None:
        migrated = migrate.migrate_aurora(self.CORPUS)
        self.assertEqual(migrate.count_borrow_keywords(migrated), 0)

    def test_migration_is_deterministic(self) -> None:
        self.assertEqual(
            migrate.migrate_aurora(self.CORPUS),
            migrate.migrate_aurora(self.CORPUS),
        )


class MarkdownTests(unittest.TestCase):
    def test_only_fenced_and_inline_code_is_migrated(self) -> None:
        original = source(
            """
            Aurora used to spell a shared loan `borrow T`. Borrowing is still
            the word we use when teaching it.

            ```python
            def read(borrow self) -> int32:
                return 0
            ```
            """
        )
        migrated = migrate.migrate_markdown(original)
        self.assertIn("Borrowing is still", migrated)
        self.assertIn("the word we use when teaching it.", migrated)
        self.assertIn("a shared loan `T`", migrated)
        self.assertIn("def read(self) -> int32:", migrated)

    def test_markdown_migration_is_idempotent(self) -> None:
        original = source(
            """
            ```python
            def f(value: borrow mut String):
                pass
            ```
            """
        )
        once = migrate.migrate_markdown(original)
        self.assertEqual(migrate.migrate_markdown(once), once)


class ManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)
        self.target = self.root / "sample.au"
        self.target.write_text("def read(borrow self) -> int32:\n    return 0\n")
        self.manifest_path = self.root / "manifest.json"

    def build_manifest(self) -> dict:
        manifest = migrate.build_manifest(self.root, [self.target])
        self.manifest_path.write_text(json.dumps(manifest, indent=2))
        return manifest

    def test_manifest_records_before_and_after_hashes(self) -> None:
        manifest = self.build_manifest()
        entry = manifest["files"][0]
        self.assertEqual(entry["path"], "sample.au")
        self.assertNotEqual(entry["before"], entry["after"])
        self.assertEqual(len(entry["before"]), 64)

    def test_apply_rewrites_every_manifest_entry(self) -> None:
        manifest = self.build_manifest()
        changed = migrate.apply_manifest(self.root, manifest)
        self.assertEqual(changed, ["sample.au"])
        self.assertEqual(
            self.target.read_text(), "def read(self) -> int32:\n    return 0\n"
        )

    def test_second_apply_changes_nothing(self) -> None:
        manifest = self.build_manifest()
        migrate.apply_manifest(self.root, manifest)
        self.assertEqual(migrate.apply_manifest(self.root, manifest), [])

    def test_apply_refuses_a_file_that_changed_since_the_manifest(self) -> None:
        manifest = self.build_manifest()
        self.target.write_text("def read(borrow self) -> int32:\n    return 1\n")
        with self.assertRaises(migrate.HashMismatch) as caught:
            migrate.apply_manifest(self.root, manifest)
        self.assertIn("sample.au", str(caught.exception))

    def test_check_reports_pending_files_without_writing(self) -> None:
        manifest = self.build_manifest()
        before = self.target.read_text()
        self.assertEqual(migrate.check_manifest(self.root, manifest), ["sample.au"])
        self.assertEqual(self.target.read_text(), before)

    def test_check_is_clean_after_apply(self) -> None:
        manifest = self.build_manifest()
        migrate.apply_manifest(self.root, manifest)
        self.assertEqual(migrate.check_manifest(self.root, manifest), [])

    def test_manifest_omits_files_the_migration_would_not_change(self) -> None:
        unchanged = self.root / "clean.au"
        unchanged.write_text("def read(self) -> int32:\n    return 0\n")
        manifest = migrate.build_manifest(self.root, [self.target, unchanged])
        self.assertEqual([e["path"] for e in manifest["files"]], ["sample.au"])

    def test_manifest_entries_are_sorted_for_determinism(self) -> None:
        second = self.root / "aaa.au"
        second.write_text("def f(v: borrow String):\n    pass\n")
        manifest = migrate.build_manifest(self.root, [self.target, second])
        self.assertEqual([e["path"] for e in manifest["files"]], ["aaa.au", "sample.au"])


if __name__ == "__main__":
    unittest.main()
