#!/usr/bin/env python3
"""Behavioral tests for the ADR-0022 inventory evidence."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path, PurePosixPath


SCRIPT = Path(__file__).with_name("capability_inventory.py")
SPEC = importlib.util.spec_from_file_location("capability_inventory", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
inventory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inventory)


def named(name: str, *args: dict) -> dict:
    return {"name": name, "args": list(args), "span": {"line": 1, "column": 1}}


def tuple_type(*elements: dict) -> dict:
    return {"elements": list(elements), "span": {"line": 1, "column": 1}}


class MaintainedSourcePolicyTests(unittest.TestCase):
    def test_only_generated_or_dependency_trees_are_excluded(self) -> None:
        self.assertEqual(
            inventory.maintained_exclusion_reason(
                PurePosixPath("docs/.vitepress/dist/index.md")
            ),
            "generated VitePress output",
        )
        self.assertEqual(
            inventory.maintained_exclusion_reason(
                PurePosixPath("node_modules/package/index.md")
            ),
            "installed dependency",
        )
        self.assertEqual(
            inventory.maintained_exclusion_reason(PurePosixPath("target/tmp.au")),
            "build output",
        )

        # Historical decisions and work notes still form part of the scanned
        # evidence. Their retired spellings may be classified, but the files
        # must never disappear behind a blanket directory exclusion.
        self.assertIsNone(
            inventory.maintained_exclusion_reason(
                PurePosixPath(
                    "architecture_docs/decisions/0005-method-receivers.md"
                )
            )
        )
        self.assertIsNone(
            inventory.maintained_exclusion_reason(
                PurePosixPath("work/2026-07-27-batch3-checkpoint.md")
            )
        )


class BorrowOccurrenceTests(unittest.TestCase):
    def test_aurora_occurrences_are_token_aware_and_have_locations(self) -> None:
        source = """\
def inspect(value: borrow String):
    # borrow mut String
    text = "borrow String"
    borrowed = value
"""
        records = inventory.borrow_occurrence_records(
            PurePosixPath("example.au"), source, "aurora"
        )
        self.assertEqual(
            [(record["line"], record["column"]) for record in records],
            [(1, 20)],
        )

    def test_markdown_occurrences_only_include_code(self) -> None:
        source = """\
Borrow is useful prose.

`borrow self`

```aurora
def rename(borrow mut self):
    pass
```
"""
        records = inventory.borrow_occurrence_records(
            PurePosixPath("guide.md"), source, "markdown"
        )
        self.assertEqual([record["line"] for record in records], [3, 6])


class CopyClassificationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.declarations = inventory.collect_type_declarations(
            {
                "items": [
                    {
                        "Class": {
                            "name": "Point",
                            "copy": True,
                            "type_params": [],
                            "fields": [
                                {"name": "x", "ty": named("int32")},
                                {"name": "y", "ty": named("int64")},
                            ],
                        }
                    },
                    {
                        "Class": {
                            "name": "Box",
                            "copy": False,
                            "type_params": [],
                            "fields": [{"name": "text", "ty": named("String")}],
                        }
                    },
                    {
                        "Enum": {
                            "name": "Flag",
                            "type_params": [],
                            "variants": [
                                {"name": "Off", "payloads": []},
                                {
                                    "name": "On",
                                    "payloads": [
                                        {"name": "count", "ty": named("int32")}
                                    ],
                                },
                            ],
                        }
                    },
                    {
                        "Enum": {
                            "name": "Maybe",
                            "type_params": ["T"],
                            "variants": [
                                {"name": "None", "payloads": []},
                                {
                                    "name": "Some",
                                    "payloads": [{"name": "value", "ty": named("T")}],
                                },
                            ],
                        }
                    },
                ]
            }
        )

    def assert_status(self, node: dict, expected: str) -> None:
        result = inventory.classify_type_ref(node, self.declarations)
        self.assertEqual(result["status"], expected, result)

    def test_matches_the_compiler_copy_categories(self) -> None:
        for scalar in [
            "None",
            "bool",
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "int128",
            "intsize",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "uint128",
            "uintsize",
            "float32",
            "float64",
            "Duration",
        ]:
            with self.subTest(scalar=scalar):
                self.assert_status(named(scalar), inventory.COPY)

        for handle in ["Queue", "Task"]:
            with self.subTest(handle=handle):
                self.assert_status(
                    named(handle, named("String")),
                    inventory.COPY,
                )

        self.assert_status(
            tuple_type(named("int32"), named("Option", named("Flag"))),
            inventory.COPY,
        )
        self.assert_status(
            named("Result", named("Point"), named("Maybe", named("int64"))),
            inventory.COPY,
        )
        self.assert_status(named("Point"), inventory.COPY)
        self.assert_status(named("Flag"), inventory.COPY)

    def test_move_and_unresolved_types_are_not_reported_as_copy(self) -> None:
        for move_type in [
            named("String"),
            named("Range"),
            named("Vec", named("int32")),
            named("TaskResult", named("int32")),
            named("Box"),
            tuple_type(named("int32"), named("String")),
            named("Maybe", named("String")),
        ]:
            with self.subTest(move_type=move_type):
                self.assert_status(move_type, inventory.MOVE)

        # These were false positives in the original hand-maintained list.
        self.assert_status(named("char"), inventory.UNRESOLVED)
        self.assert_status(named("Instant"), inventory.UNRESOLVED)
        self.assert_status(named("ImportedPoint"), inventory.UNRESOLVED)
        result = inventory.classify_type_ref(
            named("Maybe", named("T")),
            self.declarations,
            type_params={"T"},
        )
        self.assertEqual(result["status"], inventory.UNRESOLVED)


class AstEvidenceTests(unittest.TestCase):
    def test_records_copy_parameters_receivers_matches_and_concrete_impls(self) -> None:
        tree = {
            "items": [
                {
                    "Class": {
                        "name": "Point",
                        "copy": True,
                        "type_params": [],
                        "fields": [{"name": "x", "ty": named("int32")}],
                        "methods": [
                            {
                                "name": "x_value",
                                "receiver": "Borrow",
                                "params": [],
                                "span": {"line": 3, "column": 5},
                            },
                            {
                                "name": "shift",
                                "receiver": "BorrowMut",
                                "params": [],
                                "span": {"line": 5, "column": 5},
                            },
                        ],
                    }
                },
                {
                    "Trait": {
                        "name": "Show",
                        "type_params": [],
                        "methods": [
                            {
                                "name": "show",
                                "receiver": "Borrow",
                                "params": [],
                                "span": {"line": 9, "column": 5},
                            }
                        ],
                    }
                },
                {
                    "Impl": {
                        "trait_name": "Show",
                        "type_params": [],
                        "trait_args": [],
                        "for_type": named("int32"),
                        "methods": [
                            {
                                "name": "show",
                                "receiver": "Borrow",
                                "params": [
                                    {
                                        "name": "radix",
                                        "mode": "Default",
                                        "ty": named("int32"),
                                        "span": {"line": 13, "column": 19},
                                    }
                                ],
                                "span": {"line": 13, "column": 5},
                            }
                        ],
                        "span": {"line": 12, "column": 1},
                    }
                },
                {
                    "Function": {
                        "name": "inspect",
                        "receiver": None,
                        "params": [
                            {
                                "name": "pair",
                                "mode": "Default",
                                "ty": tuple_type(named("int32"), named("bool")),
                                "span": {"line": 17, "column": 13},
                            },
                            {
                                "name": "external",
                                "mode": "Default",
                                "ty": named("ImportedPoint"),
                                "span": {"line": 17, "column": 34},
                            },
                        ],
                        "body": [
                            {
                                "Match": {
                                    "capability": "Borrow",
                                    "scrutinee": {
                                        "kind": {"Name": "value"},
                                        "span": {"line": 18, "column": 11},
                                    },
                                    "arms": [],
                                    "span": {"line": 18, "column": 5},
                                }
                            }
                        ],
                        "span": {"line": 17, "column": 1},
                    }
                },
            ]
        }

        evidence = inventory.collect_ast_evidence(
            tree, PurePosixPath("sample.au")
        )
        self.assertEqual(
            {
                record["parameter"]
                for record in evidence["parameters"]
                if record["mode"] == "Default"
                and record["copy_classification"] == inventory.COPY
            },
            {"radix", "pair"},
        )
        self.assertEqual(
            [
                (record["function"], record["mode"])
                for record in evidence["receivers"]
            ],
            [
                ("x_value", "Borrow"),
                ("shift", "BorrowMut"),
                ("show", "Borrow"),
                ("show", "Borrow"),
            ],
        )
        self.assertEqual(
            [
                record["function"]
                for record in evidence["receivers"]
                if record["copy_classification"] == inventory.COPY
            ],
            ["x_value", "shift", "show"],
        )
        self.assertEqual(len(evidence["bare_matches"]), 1)
        self.assertEqual(
            evidence["trait_impls"][0]["concreteness"],
            inventory.CONCRETE,
        )
        self.assertEqual(
            evidence["trait_impls"][0]["target_copy_classification"],
            inventory.COPY,
        )

    def test_generic_or_imported_impls_are_not_guessed_concrete(self) -> None:
        tree = {
            "items": [
                {
                    "Trait": {
                        "name": "Map",
                        "type_params": ["T"],
                        "methods": [],
                    }
                },
                {
                    "Impl": {
                        "trait_name": "Map",
                        "type_params": [],
                        "trait_args": [named("T")],
                        "for_type": named("Vec", named("T")),
                        "methods": [],
                        "span": {"line": 4, "column": 1},
                    }
                },
                {
                    "Impl": {
                        "trait_name": "Map",
                        "type_params": [],
                        "trait_args": [named("int32")],
                        "for_type": named("ImportedBox"),
                        "methods": [],
                        "span": {"line": 7, "column": 1},
                    }
                },
            ]
        }
        evidence = inventory.collect_ast_evidence(
            tree, PurePosixPath("impls.au")
        )
        self.assertEqual(
            [record["concreteness"] for record in evidence["trait_impls"]],
            [inventory.GENERIC, inventory.UNRESOLVED],
        )


class BuiltinEvidenceTests(unittest.TestCase):
    def test_builtin_enum_variants_cannot_silently_escape_the_inventory(self) -> None:
        source = """\
pub enum BuiltinMember {
    Good,
    Braced,
    Missing,
}

Self::Good => "good() -> None",
Self::Braced => {
    "braced() -> None"
}
Self::Good => {
    BuiltinCallShape::fixed(&NO_BUILTIN_PARAMS, CallConvention::PositionalOnly)
}
Self::Braced => {
    BuiltinCallShape::fixed(&NO_BUILTIN_PARAMS, CallConvention::PositionalOnly)
}
"""
        coverage = inventory.collect_builtin_variant_coverage(source)
        self.assertEqual(
            coverage["missing_rendered_signatures"],
            ["BuiltinMember::Missing"],
        )
        self.assertEqual(
            coverage["missing_structured_call_shapes"],
            ["BuiltinMember::Missing"],
        )

    def test_rendered_signatures_expose_capability_and_copy_evidence(self) -> None:
        source = """\
Self::Inspect => "inspect(count: int64, text: String, item: own T) -> None",
Self::Shuffle => "shuffle(values: mut Vec[T]) -> None",
Self::Overload => "choose(start: int32) -> int32; choose(value) -> int32",
"""
        records = inventory.collect_rendered_builtin_signatures(
            source, PurePosixPath("call.rs")
        )
        params = [
            param
            for signature in records
            for param in signature["parameters"]
        ]
        self.assertEqual(
            [(param["name"], param["capability"]) for param in params],
            [
                ("count", "bare"),
                ("text", "bare"),
                ("item", "own"),
                ("values", "mut"),
                ("start", "bare"),
                ("value", "bare"),
            ],
        )
        self.assertEqual(
            [
                param["name"]
                for param in params
                if param["copy_classification"] == inventory.COPY
            ],
            ["count", "start"],
        )
        self.assertEqual(
            next(param for param in params if param["name"] == "value")[
                "copy_classification"
            ],
            inventory.UNRESOLVED,
        )

    def test_module_builtin_helpers_are_reported_without_semantic_guessing(self) -> None:
        source = """\
function_info(
    "io",
    "open",
    vec![
        value_param("timeout", type_ref("Duration", Vec::new())),
        own_param("path", type_ref("String", Vec::new())),
        borrow_param("value", json_value()),
    ],
    type_ref("None", Vec::new()),
)
"""
        records = inventory.collect_module_builtin_parameters(
            source, PurePosixPath("builtin_modules.rs")
        )
        self.assertEqual(
            [(record["parameter"], record["capability"]) for record in records],
            [("timeout", "bare"), ("path", "own"), ("value", "bare")],
        )
        self.assertEqual(records[0]["copy_classification"], inventory.COPY)
        self.assertEqual(records[1]["copy_classification"], inventory.MOVE)
        self.assertEqual(records[2]["copy_classification"], inventory.MOVE)

    def test_rendered_capabilities_are_compared_with_call_shape_metadata(self) -> None:
        source = """\
const COUNT_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "value", ReceiverKind::Value)];
const TEXT_PARAMS: [BuiltinParam; 1] =
    [builtin_param!(required, "text", ReceiverKind::Value)];

Self::DurationMilliseconds => "ms(value: int64) -> Duration",
Self::Consume => "consume(text: own String) -> None",

Self::DurationMilliseconds => {
    BuiltinCallShape::fixed(&COUNT_PARAMS, CallConvention::PositionalOrNamed)
}
Self::Consume => {
    BuiltinCallShape::fixed(&TEXT_PARAMS, CallConvention::PositionalOrNamed)
}
"""
        evidence = inventory.collect_builtin_capability_consistency(
            source, PurePosixPath("call.rs")
        )
        self.assertEqual(
            evidence["mismatches"],
            [
                {
                    "variant": "DurationMilliseconds",
                    "callable": "ms",
                    "parameter": "value",
                    "position": 0,
                    "rendered_capability": "bare",
                    "expected_passing": "Borrow",
                    "metadata_passing": "Value",
                    "metadata_constant": "COUNT_PARAMS",
                }
            ],
        )

    def test_missing_copy_argument_application_is_an_explicit_mismatch(self) -> None:
        call_source = """\
const TWO_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "first", ReceiverKind::Borrow),
    builtin_param!(required, "second", ReceiverKind::Borrow),
];
Self::Good => "good(first: int32, second: int32) -> None",
Self::Bad => "bad(first: int32, second: int32) -> None",
Self::Good | Self::Bad => {
    BuiltinCallShape::fixed(&TWO_PARAMS, CallConvention::PositionalOrNamed)
}
"""
        sema_source = """\
match builtin_member {
    BuiltinMember::Good => {
        self.apply_builtin_argument_passing(
            builtin_member,
            0,
            first,
            locals,
        )?;
        Ok(Type::Unit)
    },
    BuiltinMember::Bad => {
        let _ = self.type_of_expr(&first.value, locals)?;
        Ok(Type::Unit)
    },
}
"""
        evidence = inventory.collect_builtin_application_evidence(
            call_source,
            sema_source,
            PurePosixPath("call.rs"),
            PurePosixPath("sema.rs"),
        )
        self.assertEqual(
            evidence["missing_sibling_retention_applications"],
            [
                {
                    "variant": "Bad",
                    "callable": "bad",
                    "parameter": "first",
                    "position": 0,
                    "type": "int32",
                    "later_parameter_positions": [1],
                    "sema_application_positions": [],
                }
            ],
        )

    def test_centralized_overlap_helpers_cover_each_builtin_family(self) -> None:
        call_source = """\
pub enum BuiltinFunction {
    Range,
}
pub enum BuiltinAssociatedFunction {
    DurationMilliseconds,
}
pub enum BuiltinMember {
    VecSet,
}
const TWO_PARAMS: [BuiltinParam; 2] = [
    builtin_param!(required, "first", ReceiverKind::Borrow),
    builtin_param!(required, "second", ReceiverKind::BorrowMut),
];
impl BuiltinFunction {
    fn detail(self) -> &'static str {
        match self {
            Self::Range => "range(first: int32, second: mut int32) -> Range",
        }
    }
    fn call_shape(self) -> BuiltinCallShape {
        match self {
            Self::Range => BuiltinCallShape::fixed(
                &TWO_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
        }
    }
}
impl BuiltinAssociatedFunction {
    fn detail(self) -> &'static str {
        match self {
            Self::DurationMilliseconds => "Duration.ms(first: int64, second: mut int64) -> Duration",
        }
    }
    fn call_shape(self) -> BuiltinCallShape {
        match self {
            Self::DurationMilliseconds => BuiltinCallShape::fixed(
                &TWO_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
        }
    }
}
impl BuiltinMember {
    fn detail(self) -> &'static str {
        match self {
            Self::VecSet => "Vec.set(first: int32, second: mut int32) -> None",
        }
    }
    fn call_shape(self) -> BuiltinCallShape {
        match self {
            Self::VecSet => BuiltinCallShape::fixed(
                &TWO_PARAMS,
                CallConvention::PositionalOrNamed,
            ),
        }
    }
}
"""
        sema_source = """\
self.reject_builtin_function_argument_sibling_overlap(
    builtin,
    args,
    &ordered_args,
    locals,
)?;
self.reject_builtin_associated_argument_sibling_overlap(
    constructor,
    args,
    &ordered_args,
    locals,
)?;
self.reject_builtin_member_argument_sibling_overlap(
    builtin_member,
    args,
    locals,
    span,
)?;
"""
        evidence = inventory.collect_builtin_application_evidence(
            call_source,
            sema_source,
            PurePosixPath("call.rs"),
            PurePosixPath("sema.rs"),
        )
        self.assertEqual(evidence["missing_sibling_retention_applications"], [])
        self.assertTrue(evidence["bare_copy_parameter_applications"])
        self.assertTrue(
            all(
                record["sema_application_positions"] == ["all"]
                for record in evidence["bare_copy_parameter_applications"]
            )
        )


if __name__ == "__main__":
    unittest.main()
