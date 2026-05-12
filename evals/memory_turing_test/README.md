# Salehi Memory Turing Test

This benchmark evaluates whether an agent memory system has persistent,
accurate, evolving memory instead of a searchable transcript.

It covers ten categories across six real-like agent scenarios and compares:

- transcript memory
- vector memory
- summary memory
- graph memory
- Reality Graph temporal belief memory

The benchmark is deterministic. Scenario rows live in `scenarios.tsv`; the
`rg-memory-turing-test` crate parses them and emits JSONL results, Markdown
reports, and leaderboard summaries.
