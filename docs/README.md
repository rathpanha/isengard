# Isengard docs

Project hand-off for humans and AI agents. Living truth lives in topic files;
history lives only in the changelog.

| File | What it is |
| --- | --- |
| [status.md](status.md) | What works, known gaps, roadmap |
| [architecture.md](architecture.md) | Quick start, `src/` map, how pieces fit, conventions, pins |
| [design.md](design.md) | UI rules (block style, Kit-only, patterns) — requirements |
| [credits.md](credits.md) | Third-party projects and people we rely on (recognition) |
| [changelog.md](changelog.md) | Chronological history (newest first) |

Root [AGENTS.md](../AGENTS.md) and [CLAUDE.md](../CLAUDE.md) are thin stubs that
point here (Cursor / Claude tooling). Human license stance stays in
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Agent protocol

Read this file first, then [status.md](status.md) and
[architecture.md](architecture.md). For any UI change, also read
[design.md](design.md).

Isengard is a cross-platform desktop code editor in Rust on **GPUI Kit**
(`gpui-kit` — formerly "gpui-component", Longbridge, on Zed's GPUI). Use Kit
components only; do not hand-build widgets the library already provides.

### Same commit as every change

1. Update the **living** topic file(s) the change makes stale:
   - Feature / bug / TODO → [status.md](status.md)
   - Structure, conventions, deps, bootstrap → [architecture.md](architecture.md)
   - Visual or interaction rule / approved pattern → [design.md](design.md)
   - New bundled asset, vendored query, or notable direct dependency →
     [credits.md](credits.md) (plus keep the license file next to the asset)
2. Append **one** bullet to [changelog.md](changelog.md) only (newest first).

### Do not

- Put a Changelog section in status, architecture, or design.
- Duplicate the same changelog entry in two files.
- Append narrative history to status or design — edit current truth in place.
- Leave living docs stale; a stale hand-off is worse than none.
