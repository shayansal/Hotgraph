# Memory Turing Test: Evaluating Persistent Agent Memory

## Abstract

Long-running agents need memory that persists, updates, forgets, explains, and
respects permissions. The Memory Turing Test evaluates whether an agent memory
system behaves like a reliable evolving memory rather than a searchable chat
transcript. Reality Graph stores episodic, semantic, procedural, preference,
goal, plan, reflection, correction, relationship, and world-state memories as
source-backed graph events and assertions. The benchmark tests memory across
1,000-session synthetic scenarios with corrections, redactions, contradictions,
planning tasks, and tenant boundaries.

## Related Work

Vector memory and transcript summarization are common in agent frameworks but
often collapse temporal revisions into a single current summary. Memory-oriented
retrieval work such as [HippoRAG](https://arxiv.org/abs/2405.14831) motivates
graph-based multi-hop activation and long-term retrieval. Temporal knowledge
graph systems show how relationships change over time, but agent memory needs
additional lifecycle semantics: candidate, active, reinforced, superseded,
contradicted, and archived. Reality Graph frames persistent memory as a
source-backed temporal belief system with explainable retrieval paths.

## Method

The benchmark generates agent sessions in which facts, preferences, plans, and
relationships evolve. Agents observe events, retrieve memory, act, write new
memory, receive corrections, and later answer planning or recall questions.
Reality Graph records memories as events, links them to entities and sources,
and marks superseded memories without deletion. Retrieval can combine semantic
similarity, temporal filters, graph activation, source trust, and agent-specific
permissions.

## Datasets

The initial suite lives in `evals/memory_turing_test` and covers executive
assistant, coding agent, research assistant, customer-support, personal AI, and
enterprise operations scenarios. `rg-worldgen` can generate longer worlds with
hidden true state, noisy observed state, corrections, and task prompts.

## Experiments

Compare transcript memory, vector memory, rolling summary memory, graph memory,
and Reality Graph temporal belief memory. Tasks ask whether the agent remembers
facts across many sessions, revises beliefs when corrected, distinguishes old
truth from current truth, forgets or redacts when instructed, retrieves relevant
context under token budget, explains why it remembers something, uses memory in
planning, and avoids leaking one user's memory into another context.

## Ablations

- Remove memory lifecycle state.
- Remove supersession links.
- Disable temporal filters.
- Disable graph activation and use vector search only.
- Disable permissions.
- Disable source provenance.
- Disable context compression.

## Limitations

The Memory Turing Test is synthetic by design. It controls ground truth and
stress cases, but real users produce messier preferences, ambiguous corrections,
and conflicting privacy expectations. Passing the benchmark does not prove
psychological memory quality. It tests operational properties that matter for
agents: persistence, revision, retrieval, explanation, and isolation.

## Reproducibility Checklist

- Publish scenario seeds and generated session logs.
- Save all memory writes, corrections, redactions, and retrieval calls.
- Save retrieved context, memory explanations, and final answers.
- Evaluate every baseline with identical token budgets.
- Report leakage tests by tenant, user, and agent identity.
- Report both aggregate scores and per-scenario failure cases.
