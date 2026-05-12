# Adversarial Memory Evaluation

This deterministic eval suite tests whether Reality Graph memory and retrieval behavior survives hostile inputs instead of turning poisoned context into trusted memory.

The scenarios cover:

- prompt-injection documents
- malicious memory writes
- poisoned sources
- conflicting identities
- fake authority sources
- temporal spoofing
- source replay attacks
- cross-tenant leakage attempts
- tool-output manipulation
- summary poisoning

Rows live in `scenarios.tsv`. The `rg-adversarial-memory-eval` crate parses them and emits per-case JSONL results, aggregate safety metrics, and a Markdown report.
