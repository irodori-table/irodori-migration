<!-- i18n: language-switcher -->
[English](architecture.md) | [日本語](architecture.ja.md)

# Architecture

`irodori-migration` is an execution-free planning crate.

It generates plans, SQL, previews, manifests, and export streams. Host apps own
connections, credentials, scheduling, approvals, and execution.

## Flow

1. Describe source, target, keys, and columns.
2. Generate a plan.
3. Review destructive changes.
4. Compare counts and checksums.
5. Narrow mismatches by bucket.
6. Diff rows only where needed.
7. Let the host execute approved work.

## Modules

- `plan`: migration plans and SQL.
- `canonical`: cross-engine value rendering.
- `checksum`: chunked checksums and manifests.
- `schema`: schema snapshots and diffs.
- `recipe`: dry-run previews.
- `rollout`: expand/contract runbooks.
- `io`: tabular import/export.
- `export`: progress and cancellation wrapper.
- `dialect`: minimal SQL helpers.
