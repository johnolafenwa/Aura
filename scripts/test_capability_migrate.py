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
        migrated = migrate.migrate_aura(original)
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
        self.assertEqual(migrate.migrate_aura(original), original)

    def test_identifiers_containing_borrow_are_not_rewritten(self) -> None:
        original = source(
            """
            def check(borrowed: String, reborrow: String) -> String:
                borrow_count = 1
                return borrowed
            """
        )
        migrated = migrate.migrate_aura(original)
        self.assertIn("borrowed: String", migrated)
        self.assertIn("reborrow: String", migrated)
        self.assertIn("borrow_count = 1", migrated)


class ReceiverTests(unittest.TestCase):
    def test_shared_receiver_loses_the_keyword(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def read(borrow self) -> int32:\n"),
            "def read(self) -> int32:\n",
        )

    def test_mutable_receiver_becomes_mut_self(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def bump(borrow mut self):\n"),
            "def bump(mut self):\n",
        )

    def test_owning_receiver_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def close(own self):\n"),
            "def close(own self):\n",
        )

    def test_already_bare_receiver_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def read(self) -> int32:\n"),
            "def read(self) -> int32:\n",
        )


class ParameterTests(unittest.TestCase):
    def test_bare_copy_parameter_is_flagged_for_snapshot_review(self) -> None:
        result = migrate.analyze_aura(
            "def add(left: int32, right: String):\n",
            path="sample.au",
        )
        self.assertEqual(result.text, "def add(left: int32, right: String):\n")
        self.assertEqual(
            [
                (record["kind"], record["line"], record["action"])
                for record in result.occurrences
            ],
            [("bare_copy_parameter", 1, "review_required")],
        )
        self.assertEqual(len(result.findings), 1)
        self.assertIn("call-site review", result.findings[0]["message"])

    def test_structural_copy_parameter_is_also_inspected(self) -> None:
        self.assertEqual(
            migrate.migrate_aura(
                "def inspect(value: Option[(int32, bool)]):\n"
            ),
            "def inspect(value: Option[(int32, bool)]):\n",
        )

    def test_already_explicit_and_noncopy_parameters_are_unchanged(self) -> None:
        original = (
            "def inspect(a: own int32, b: mut int32, c: String, d: T):\n"
        )
        self.assertEqual(migrate.migrate_aura(original), original)

    def test_shared_parameter_loses_the_keyword(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def f(value: borrow String):\n"),
            "def f(value: String):\n",
        )

    def test_mutable_parameter_becomes_mut(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def f(value: borrow mut String):\n"),
            "def f(value: mut String):\n",
        )

    def test_mutable_tuple_parameter_keeps_its_type(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def f(value: borrow mut (String,)):\n"),
            "def f(value: mut (String,)):\n",
        )

    def test_labelled_parameter_drops_the_retired_label(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def f(value: borrow[src] String):\n"),
            "def f(value: String):\n",
        )

    def test_owning_parameter_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("def f(value: own String):\n"),
            "def f(value: own String):\n",
        )


class ReturnAnnotationTests(unittest.TestCase):
    def test_labelled_borrowed_return_becomes_an_ordinary_return(self) -> None:
        result = migrate.analyze_aura(
            "def score_ref(user: borrow User) -> borrow[user] int32:\n",
            path="score.au",
        )
        self.assertEqual(result.text, "def score_ref(user: User) -> int32:\n")
        self.assertEqual(result.findings, [])
        self.assertEqual(result.occurrences[0]["action"], "ordinary_owned_return")

    def test_noncopy_borrowed_return_is_not_blindly_rewritten(self) -> None:
        original = "def pick(a: borrow String) -> borrow String:\n"
        result = migrate.analyze_aura(original, path="pick.au")
        self.assertEqual(result.text, "def pick(a: String) -> borrow String:\n")
        self.assertEqual(len(result.findings), 1)
        self.assertEqual(result.findings[0]["kind"], "borrowed_return_redesign")
        self.assertEqual(result.findings[0]["line"], 1)
        self.assertIn("owned result, clone, index, handle, or owner operation", result.findings[0]["message"])

    def test_unresolved_borrowed_return_requires_review(self) -> None:
        result = migrate.analyze_aura(
            "def pick[T](a: borrow T) -> borrow T:\n",
            path="pick.au",
        )
        self.assertEqual(result.text, "def pick[T](a: T) -> borrow T:\n")
        self.assertEqual(result.findings[0]["classification"], "unresolved")


class LoopTests(unittest.TestCase):
    def test_shared_iteration_becomes_bare(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("for value in borrow values:\n"),
            "for value in values:\n",
        )

    def test_mutable_iteration_becomes_mut(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("for value in borrow mut values:\n"),
            "for value in mut values:\n",
        )

    def test_owning_iteration_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("for value in own values:\n"),
            "for value in own values:\n",
        )

    def test_range_capability_modifiers_are_stripped(self) -> None:
        # ADR-0022's additional ruling: range yields copy values, so `mut` and
        # `own` are rejected rather than preserved as no-ops.
        self.assertEqual(
            migrate.migrate_aura("for index in mut range(0, 3):\n"),
            "for index in range(0, 3):\n",
        )
        self.assertEqual(
            migrate.migrate_aura("for index in own range(0, 3):\n"),
            "for index in range(0, 3):\n",
        )


class MatchTests(unittest.TestCase):
    def test_shared_match_becomes_bare(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("match borrow value:\n"),
            "match value:\n",
        )

    def test_mutable_match_becomes_match_mut(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("match borrow mut value:\n"),
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
        self.assertIn("match own holder:", migrate.migrate_aura(original))

    def test_bare_match_over_a_temporary_stays_bare_and_is_flagged(self) -> None:
        # The call result has no surviving owner, but nested payload transfer
        # can still need `own`, so the tool must preserve and flag it.
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
        result = migrate.analyze_aura(original, path="temporary.au")
        self.assertIn("match compute():", result.text)
        self.assertNotIn("match own", result.text)
        self.assertEqual(result.findings[0]["kind"], "bare_match")

    def test_bare_match_without_bindings_gains_own(self) -> None:
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
        self.assertIn("match own holder:", migrate.migrate_aura(original))

    def test_every_bare_place_match_is_recorded_per_occurrence(self) -> None:
        original = source(
            """
            match holder:
                case _:
                    pass
            match holder.value:
                case _:
                    pass
            match items[index]:
                case _:
                    pass
            match compute():
                case _:
                    pass
            """
        )
        result = migrate.analyze_aura(original, path="matches.au")
        self.assertIn("match own holder:", result.text)
        self.assertIn("match own holder.value:", result.text)
        self.assertIn("match own items[index]:", result.text)
        self.assertIn("match compute():", result.text)
        self.assertEqual(
            [
                (record["line"], record["classification"], record["action"])
                for record in result.occurrences
                if record["kind"] == "bare_match"
            ],
            [
                (1, "place", "insert_own"),
                (4, "place", "insert_own"),
                (7, "place", "insert_own"),
                (10, "temporary", "review_required"),
            ],
        )

    def test_match_expressions_receive_the_same_place_preservation(self) -> None:
        original = source(
            """
            def choose(value: Option[int32]) -> int32:
                return match value:
                    case Option.Some(inner): inner
                    case Option.None: 0

            def temporary() -> int32:
                return match compute():
                    case Option.Some(inner): inner
                    case Option.None: 0
            """
        )
        result = migrate.analyze_aura(original, path="expressions.au")
        self.assertIn("return match own value:", result.text)
        self.assertIn("return match compute():", result.text)
        self.assertEqual(
            [
                (record["line"], record["classification"], record["action"])
                for record in result.occurrences
                if record["kind"] == "bare_match"
            ],
            [
                (2, "place", "insert_own"),
                (7, "temporary", "review_required"),
            ],
        )

    def test_explicitly_shared_match_never_gains_own(self) -> None:
        # `match borrow X` was already shared. Collapsing it to `match X` and
        # then annotating it `own` would silently make it consuming, which is
        # the opposite of what the source said.
        original = source(
            """
            def main():
                match borrow holder:
                    case Option.Some(value):
                        print(value)
                    case Option.None:
                        pass
            """
        )
        migrated = migrate.migrate_aura(original)
        self.assertIn("match holder:", migrated)
        self.assertNotIn("match own", migrated)

    def test_explicitly_mutable_match_never_gains_own(self) -> None:
        original = source(
            """
            def main():
                match borrow mut holder:
                    case Option.Some(value):
                        print(value)
                    case Option.None:
                        pass
            """
        )
        migrated = migrate.migrate_aura(original)
        self.assertIn("match mut holder:", migrated)
        self.assertNotIn("match own", migrated)

    def test_match_own_is_unchanged(self) -> None:
        self.assertEqual(
            migrate.migrate_aura("match own value:\n"),
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

        def scan(values: borrow mut Vec[String], limit: own int32):
            for value in borrow mut values:
                print(value)
            for index in mut range(0, limit):
                print(index)
            match compute():
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
        once = migrate.migrate_aura(self.CORPUS)
        twice = migrate.migrate_aura(once)
        self.assertEqual(once, twice)

    def test_migrated_source_has_no_borrow_keyword_left(self) -> None:
        migrated = migrate.migrate_aura(self.CORPUS)
        self.assertEqual(migrate.count_borrow_keywords(migrated), 0)

    def test_migration_is_deterministic(self) -> None:
        self.assertEqual(
            migrate.migrate_aura(self.CORPUS),
            migrate.migrate_aura(self.CORPUS),
        )


class MarkdownTests(unittest.TestCase):
    def test_only_fenced_and_inline_code_is_migrated(self) -> None:
        original = source(
            """
            Aura used to spell a shared loan `borrow T`. Borrowing is still
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

    def test_manifest_is_a_per_occurrence_semantic_ledger(self) -> None:
        self.target.write_text(
            source(
                """
                def inspect(value: int32):
                    match state:
                        case Flag.Ready:
                            pass
                """
            )
        )
        manifest = self.build_manifest()
        self.assertEqual(manifest["version"], 2)
        self.assertEqual(
            [
                (record["kind"], record["line"], record["action"])
                for record in manifest["semantic_occurrences"]
            ],
            [
                ("bare_copy_parameter", 1, "review_required"),
                ("bare_match", 2, "insert_own"),
            ],
        )
        self.assertEqual(len(manifest["findings"]), 1)
        self.assertEqual(manifest["findings"][0]["kind"], "bare_copy_parameter")

    def test_manifest_exposes_nonmechanical_borrowed_return_findings(self) -> None:
        self.target.write_text(
            "def expose(value: borrow String) -> borrow String:\n"
        )
        manifest = self.build_manifest()
        self.assertEqual(len(manifest["findings"]), 1)
        self.assertEqual(
            manifest["findings"][0]["kind"], "borrowed_return_redesign"
        )
        self.assertEqual(
            manifest["semantic_occurrences"][0]["action"],
            "redesign_required",
        )
        self.assertEqual(
            migrate.unresolved_semantic_findings(manifest),
            manifest["findings"],
        )
        manifest["findings"][0]["status"] = "resolved"
        manifest["findings"][0]["resolution"] = "return an owned clone"
        self.assertEqual(migrate.unresolved_semantic_findings(manifest), [])

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

    def test_check_accepts_a_clean_post_migration_edit(self) -> None:
        manifest = self.build_manifest()
        migrate.apply_manifest(self.root, manifest)
        self.target.write_text("def read(self) -> int32:\n    return 1\n")
        self.assertEqual(migrate.check_manifest(self.root, manifest), [])

    def test_check_rejects_post_migration_drift_that_restores_old_syntax(self) -> None:
        manifest = self.build_manifest()
        migrate.apply_manifest(self.root, manifest)
        self.target.write_text("def read(borrow self) -> int32:\n    return 1\n")
        with self.assertRaises(migrate.HashMismatch) as caught:
            migrate.check_manifest(self.root, manifest)
        self.assertIn("still contains retired capability syntax", str(caught.exception))

    def test_check_rejects_a_missing_manifest_entry(self) -> None:
        manifest = self.build_manifest()
        self.target.unlink()
        with self.assertRaises(migrate.HashMismatch) as caught:
            migrate.check_manifest(self.root, manifest)
        self.assertIn("listed in the manifest but missing", str(caught.exception))

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


class RetiredSyntaxGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)

    def write(self, relative: str, text: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source(text))
        return path

    def test_clean_aura_and_explanatory_borrow_words_pass(self) -> None:
        aura = self.write(
            "example.au",
            '''
            # A shared borrow does not consume the value.
            def explain(value: String):
                note = "borrow mut T is retired syntax"
                print(value)
            ''',
        )
        manual = self.write(
            "manual.md",
            """
            A bare parameter borrows a non-copy value. The borrow ends at the
            call boundary, and the borrow checker reports overlapping access.
            """,
        )
        self.assertEqual(
            migrate.find_retired_syntax(self.root, [aura, manual], {}),
            [],
        )

    def test_retired_keyword_in_aura_source_fails(self) -> None:
        aura = self.write(
            "example.au",
            """
            def read(value: borrow String):
                print(value)
            """,
        )
        findings = migrate.find_retired_syntax(self.root, [aura], {})
        self.assertEqual(len(findings), 1)
        self.assertIn("example.au:1", findings[0])
        self.assertIn("retired `borrow` keyword", findings[0])

    def test_aura_retirement_fixture_requires_an_exact_counted_exemption(self) -> None:
        fixture = self.write(
            "fixtures/retired.au",
            """
            def read(value: borrow String):
                print(value)
            """,
        )
        allowlist = {
            "fixtures/retired.au": {
                "borrow_keywords": 1,
                "reason": "compiler replacement diagnostic fixture",
            }
        }
        self.assertEqual(
            migrate.find_retired_syntax(self.root, [fixture], allowlist),
            [],
        )
        fixture.write_text(
            "def read(value: borrow String, other: borrow String):\n    pass\n"
        )
        findings = migrate.find_retired_syntax(
            self.root,
            [fixture],
            allowlist,
        )
        self.assertTrue(
            any("allowlist expects 1 retired keyword token, found 2" in f for f in findings)
        )

    def test_stale_markdown_source_spelling_fails_but_retirement_teaching_passes(self) -> None:
        stale = self.write(
            "stale.md",
            """
            To advance the stream, accept `rng: borrow mut random.Rng`.
            """,
        )
        teaching = self.write(
            "teaching.md",
            """
            The old spelling `value: borrow mut T` was removed; write
            `value: mut T` instead.
            """,
        )
        findings = migrate.find_retired_syntax(
            self.root,
            [stale, teaching],
            {},
        )
        self.assertEqual(len(findings), 1)
        self.assertIn("stale.md:1", findings[0])

    def test_stale_multiline_inline_code_is_detected(self) -> None:
        manual = self.write(
            "manual.md",
            """
            A function that advances a stream takes `rng: borrow mut
            random.Rng`.
            """,
        )
        findings = migrate.find_retired_syntax(self.root, [manual], {})
        self.assertEqual(len(findings), 1)
        self.assertIn("manual.md:1", findings[0])

    def test_stale_indented_markdown_code_is_detected(self) -> None:
        manual = self.write(
            "manual.md",
            """
            Example:

                def update(value: borrow mut Counter):
                    pass
            """,
        )
        findings = migrate.find_retired_syntax(self.root, [manual], {})
        self.assertEqual(len(findings), 1)
        self.assertIn("manual.md:3", findings[0])

    def test_stale_diagnostic_and_rust_message_fail(self) -> None:
        diagnostic = self.write(
            "sample.diag",
            """
            error[AU3002]: cannot start `match borrow mut` here
            """,
        )
        rust = self.write(
            "sample.rs",
            '''
            let message = "parameter is declared `borrow mut`";
            ''',
        )
        findings = migrate.find_retired_syntax(
            self.root,
            [diagnostic, rust],
            {},
        )
        self.assertEqual(len(findings), 2)
        self.assertTrue(any("sample.diag:1" in f for f in findings))
        self.assertTrue(any("sample.rs:1" in f for f in findings))

    def test_parser_retirement_diagnostic_and_internal_rust_comment_pass(self) -> None:
        rust = self.write(
            "parser.rs",
            '''
            // The old AST represented `borrow mut` with a receiver mode.
            let message = "`borrow mut T` was removed; write `mut T`";
            ''',
        )
        self.assertEqual(
            migrate.find_retired_syntax(self.root, [rust], {}),
            [],
        )

    def test_historical_work_notes_and_adrs_are_not_current_syntax(self) -> None:
        work = self.write(
            "work/2026-01-01-old.md",
            "The implementation used `value: borrow mut T`.\n",
        )
        adr = self.write(
            "architecture_docs/decisions/0006-old.md",
            "The accepted spelling was `value: borrow mut T`.\n",
        )
        self.assertEqual(
            migrate.find_retired_syntax(self.root, [work, adr], {}),
            [],
        )


if __name__ == "__main__":
    unittest.main()
