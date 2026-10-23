#!/usr/bin/env python3
"""Validate the Sequon JSON contract.

This script owns the JSON Schema validation gate. It is run by CI (after
installing the jsonschema package) and can be run locally with:

    python3 -m pip install "jsonschema>=4.23"
    python3 tools/validate_contract.py

It deliberately does not use a Rust JSON Schema crate: keeping the validator in
Python keeps the Rust dependency tree free of the LGPL-licensed r-efi chain that
jsonschema's getrandom dependency would otherwise add to Cargo.lock.

Covered here:
- both schemas are valid JSON Schema 2020-12 documents with the expected $id;
- every schemas/examples/*.json validates against the schema named by its
  "schema" marker;
- every committed golden under testdata/expected/ validates against the record
  schema;
- negative cases prove the schemas reject malformed documents;
- contentEncoding stays an annotation (invalid base64 is still schema-valid).

Cross-field invariants (topology-aware segment bounds, length == sequence
length, digest recomputation) are owned by Record::validate() and sequon_algo
and are covered by the Rust tests, not here. The segment bound is deliberately
absent from the schema negatives because it depends on the record topology,
which JSON Schema cannot relate to a segment's own fields: a linear record keeps
0 <= start <= end <= length, while on a circular record a zero-length site keeps
start = end <= length and a spanning segment may carry end > length by at most
one wrap (end <= start + length). A schema-level check would reject the legal
unwrapped circular form.
"""

from __future__ import annotations

import copy
import json
import pathlib
import sys

try:
    from jsonschema import Draft202012Validator
except ImportError:  # pragma: no cover - environment guard
    sys.exit(
        "jsonschema is required: python3 -m pip install 'jsonschema>=4.23' "
        "(CI installs it before running this script)"
    )

ROOT = pathlib.Path(__file__).resolve().parent.parent
RECORD_ID = "sequon/record/1"
REPORT_ID = "sequon/import-report/1"
RECORD_SCHEMA = ROOT / "schemas" / "sequon-record-1.schema.json"
REPORT_SCHEMA = ROOT / "schemas" / "import-report-1.schema.json"
EXAMPLES = ROOT / "schemas" / "examples"
GOLDENS = ROOT / "testdata" / "expected"

failures: list[str] = []


def load(path: pathlib.Path) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read {path}: {error}")


def fail(message: str) -> None:
    failures.append(message)


def expect_valid(validator: Draft202012Validator, instance: object, label: str) -> None:
    errors = sorted(validator.iter_errors(instance), key=lambda error: list(error.path))
    for error in errors:
        fail(f"{label}: {error.message} at {list(error.path)}")


def expect_invalid(validator: Draft202012Validator, instance: object, label: str) -> None:
    if validator.is_valid(instance):
        fail(f"{label}: schema accepted a document it must reject")


def segment(start: int, end: int) -> dict:
    return {"start": start, "end": end, "start_partial": False, "end_partial": False}


def with_location(record: dict, location: dict) -> dict:
    document = copy.deepcopy(record)
    document["features"].append(
        {"key": "misc_feature", "location": location, "qualifiers": []}
    )
    return document


def main() -> int:
    record_schema = load(RECORD_SCHEMA)
    report_schema = load(REPORT_SCHEMA)

    for name, schema, expected_id in (
        ("record", record_schema, "urn:sequon:schema:record:1"),
        ("report", report_schema, "urn:sequon:schema:import-report:1"),
    ):
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            fail(f"{name} schema: unexpected $schema")
        if schema.get("$id") != expected_id:
            fail(f"{name} schema: unexpected $id {schema.get('$id')!r}")
    Draft202012Validator.check_schema(record_schema)
    Draft202012Validator.check_schema(report_schema)
    record_validator = Draft202012Validator(record_schema)
    report_validator = Draft202012Validator(report_schema)

    examples = 0
    record_example: dict | None = None
    report_example: dict | None = None
    for path in sorted(EXAMPLES.glob("*.json")):
        document = load(path)
        marker = document.get("schema") if isinstance(document, dict) else None
        if marker == RECORD_ID:
            expect_valid(record_validator, document, str(path))
            record_example = document
        elif marker == REPORT_ID:
            expect_valid(report_validator, document, str(path))
            report_example = document
        else:
            fail(f"{path}: unsupported schema marker {marker!r}")
        examples += 1
    if record_example is None or report_example is None:
        fail("schemas/examples must contain one record and one report example")

    goldens = 0
    for path in sorted(GOLDENS.glob("*.json")):
        expect_valid(record_validator, load(path), str(path))
        goldens += 1

    if record_example is not None:
        base = record_example
        missing = copy.deepcopy(base)
        del missing["sequence"]
        expect_invalid(record_validator, missing, "record: missing sequence")
        zero_length = copy.deepcopy(base)
        zero_length["length"] = 0
        expect_invalid(record_validator, zero_length, "record: length 0")
        expect_invalid(
            record_validator,
            with_location(base, {"operator": "span", "strand": 1, "segments": [segment(-1, 4)]}),
            "record: negative segment start",
        )
        expect_invalid(
            record_validator,
            with_location(
                base,
                {"operator": "span", "strand": 1, "segments": [segment(0, 4), segment(8, 12)]},
            ),
            "record: span with two segments",
        )
        bad_digest = copy.deepcopy(base)
        bad_digest["digests"]["sequence"] = "sha256:NOT-A-DIGEST"
        expect_invalid(record_validator, bad_digest, "record: malformed digest")
        bad_kind = copy.deepcopy(base)
        bad_kind["kind"] = "plasmid"
        expect_invalid(record_validator, bad_kind, "record: unknown kind")
        unknown = copy.deepcopy(base)
        unknown["unexpected_field"] = True
        expect_invalid(record_validator, unknown, "record: unknown top-level property")

    if report_example is not None:
        base = report_example
        missing = copy.deepcopy(base)
        del missing["diagnostics"]
        expect_invalid(report_validator, missing, "report: missing diagnostics")
        negative_bytes = copy.deepcopy(base)
        negative_bytes["source_bytes"] = -1
        expect_invalid(report_validator, negative_bytes, "report: negative source_bytes")
        bad_digest = copy.deepcopy(base)
        bad_digest["source_digest"] = "sha256:ABC"
        expect_invalid(report_validator, bad_digest, "report: malformed digest")
        bad_severity = copy.deepcopy(base)
        bad_severity["diagnostics"] = [
            {"code": "GB_SEQUENCE_NORMALIZED", "severity": "fatal", "message": "bad"}
        ]
        expect_invalid(report_validator, bad_severity, "report: unknown severity")
        unknown = copy.deepcopy(base)
        unknown["unexpected_field"] = True
        expect_invalid(report_validator, unknown, "report: unknown top-level property")
        bad_position = copy.deepcopy(base)
        bad_position["diagnostics"] = [
            {
                "code": "GB_LOCATION_MALFORMED",
                "severity": "error",
                "message": "genbank position without a line",
                "position": {"kind": "genbank", "offset": 10},
            }
        ]
        expect_invalid(report_validator, bad_position, "report: GenBank position without line")

        annotation_only = copy.deepcopy(base)
        annotation_only["preserved"] = [
            {"what": "unknown_packet", "encoding": "base64", "data": "not base64!!!"}
        ]
        expect_valid(
            report_validator,
            annotation_only,
            "report: contentEncoding must remain annotation-only",
        )

    if failures:
        print(f"contract validation FAILED ({len(failures)} problem(s)):", file=sys.stderr)
        for problem in failures:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(
        f"contract validation ok: {examples} examples, {goldens} goldens, "
        "negative and annotation cases"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
