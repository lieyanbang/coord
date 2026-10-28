# Emote correction supplement version 2

This format is an offline implementation, not deployment or publication authority.
The signed recovery policy selects the canonical JSON supplement by its SHA-256;
the existing broker also binds the failed action, event, accepted result and its
stored bytes. The supplement uses `schema_version: 2`, `status: "APPROVED"`, a
positive integer `revision`, a UUID `correction_id`, and a nonempty `author`.
It repeats `batch_id`, `task_id`, `original_action_id`, `original_event_id`,
`group_id`, `accepted_result_sha256`, and `accepted_result_storage_sha256`.

`changes` is a nonempty array of `{ "field", "before", "after" }` entries.
Each field appears once and is one of `characters`, `uncertain`, or
`general_notes`. The whole prior field must match; the replacement is validated
against the original task and result schema. This allows regrouping images,
changing candidates, and recording unresolved observations without modifying
the accepted result. `evidence` lists `{ "path", "sha256" }` for every source
image and every reference used by the effective result. Paths are relative to
the business root and limited to the task group and `refs/`.

`qa` names a file below `qa/` in the supplement directory, its byte `sha256`,
and its `reviewer`. That JSON file has `schema_version: 1`, `status: "APPROVED"`,
`reviewer`, `correction_id`, `group_id`, `accepted_result_sha256`, and
`effective_result_sha256`. The reviewer must differ from the correction author.
This binds the actual QA record to the exact full effective value. Distinct
labels and hashes alone do not prove an independent visual review occurred.

The publisher writes an immutable wrapper under `results/.corrections/<group>.json`
and preserves any existing accepted result, receipt, and v2 review. If neither
result nor receipt exists, it creates a pair containing the accepted result and
its broker-confirmed receipt. An exclusive group lock serializes correction
writers. A crash or failure after writes can leave `<group>.lock`, an explicit
uncertain state requiring operator reconciliation. Identical replays succeed;
different versions conflict instead of overwriting. Create the correction and
QA directories in an isolated deployment before using this path.

The updated broker supplies successful recovery bindings in batch proofs. The
v2 index publisher refuses a missing, extra, or mismatched correction when that
proof is present, validates the QA and source files again, and records original
and effective hashes plus correction and QA provenance. Legacy v1/v2 results
without corrections remain readable. Existing published index files are never
overwritten; a changed index conflicts and needs a separately authorized versioned
publication strategy.

The seven historical QA files currently attest hashes of their reviewed
fine-grained candidate and overlay, not `effective_result_sha256` in this
format. Their conversion and the relationship to an exact effective result
require actual evidence and independent verification; synthesizing a QA
attestation from their PASS labels is not valid.

For correction results only, an empty checked reference list is permitted when
the character is unknown, has no match, and has no candidates; uncertain entries
with no checked references must also have no candidates. This records missing
reference inspection without inheriting unrelated evidence or asserting identity.
The ordinary submission and legacy validation rules remain unchanged.
