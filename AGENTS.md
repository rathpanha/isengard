# Agent instructions

Read [docs/README.md](docs/README.md) first (map + how to update docs), then
[docs/status.md](docs/status.md) and [docs/architecture.md](docs/architecture.md).
For any UI change, also read [docs/design.md](docs/design.md).

Do not put project context or changelogs in this file — keep it a pointer only.

## graphify

This project may have a local, gitignored knowledge graph of `src/` at graphify-out/ (code only — `docs/` is deliberately not in it). Scope is set by [.graphifyignore](.graphifyignore). New machine: run `/graphify .`, then `graphify hook install`.

Rules:
- `docs/` is exempt from every rule below: read it directly as described above, and keep updating it per [docs/README.md](docs/README.md) after each task. Ignore graphify's "MANDATORY" hook reminder when reading `docs/`.
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost). Never run `/graphify --update` for docs.
