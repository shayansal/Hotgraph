# Reality Gym: Training Agents in Noisy Dynamic Worlds

## Abstract

Reality Gym is a training and evaluation environment for agents that must act in
worlds with noisy evidence, hidden state, delayed consequences, memory updates,
and changing relationships. Agents observe, retrieve memory, reason, act, write
memory, trigger world updates, and receive reward signals. Reality Graph provides
the state substrate, source-backed evidence, causal links, and evaluation oracle.

## Related Work

Agent benchmarks often emphasize tool use, planning, or single-turn QA. Temporal
graph benchmarks emphasize dynamic graph prediction. Reality Gym combines these
directions by making persistent memory and dynamic world reconstruction part of
the task loop. It also complements graph retrieval work by testing whether
retrieved context leads to better actions, not merely better answers.

## Method

`reality-gym` defines `AgentEnvironment`, `Observation`, `Action`, `MemoryWrite`,
`WorldUpdate`, `RewardSignal`, and `EvaluationOracle`. `rg-worldgen` creates
companies, people, relationships, events, documents, emails, contracts,
meetings, contradictions, rumors, policies, causal chains, and agent tasks. The
graph exposes only observed state, while the oracle retains hidden true state for
evaluation.

## Datasets

Scenario families include company management, contract negotiation, fraud
investigation, research coordination, long-running codebase debugging,
customer-success account management, geopolitical crisis tracking, and personal
assistant memory. Each world includes ground truth, noisy evidence, source
documents, graph assertions, candidate contradictions, and task outcomes.

## Experiments

Compare agents with no memory, transcript memory, vector memory, summary memory,
graph memory, and full Reality Graph memory/retrieval/simulation. Tasks measure
whether agents gather missing information, avoid acting on stale evidence,
revise plans after corrections, detect risky actions, and use memory without
leaking private context. Metrics include task success, evidence use,
hallucinated-action rate, unsafe-memory-use rate, reward, and latency.

## Ablations

- Remove hidden state and make all facts directly observable.
- Remove noisy or adversarial sources.
- Disable memory writes.
- Disable causal links.
- Disable counterfactual simulation.
- Disable policy checks.
- Disable delayed consequences.

## Limitations

Synthetic worlds can overfit to benchmark structure. Reward functions may favor
conservative agents unless risk and progress are balanced. Generated documents
may lack the distributional complexity of real enterprise corpora. Multi-agent
settings need careful permission modeling to avoid benchmark artifacts.

## Reproducibility Checklist

- Publish world schemas, generation seeds, and hidden-state manifests.
- Save agent observations, tool calls, retrieved context, memory writes, actions,
  and rewards.
- Report model, prompt, tool policy, and context budget.
- Release oracle grading code and task definitions.
- Report per-scenario failures and aggregate scores.
- Separate training worlds from evaluation worlds.
