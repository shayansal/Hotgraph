# Model-Native Context Compilation

Reality Kernel does not hand an LLM random chunks. It compiles a compact,
source-backed context object for a specific task, agent, time window,
permission scope, token budget, and risk level.

## Input

`ModelContextRequest` contains:

```text
task
agent identity
current goal
valid_at
known_at
permission scope
token budget
risk level
```

The compiler treats these fields as execution constraints, not prompt hints.

## Output

`CompiledModelContext` returns:

```text
evidence_pack
current_belief_state
relevant_memories
contradictions
missing_information
safe_assumptions
recommended_actions
permission_filtered_atoms
warnings
```

This object is designed to be fed to a model or agent runtime. It says what
matters, why it matters, how we know, when it was true, what contradicts it, and
what is missing.

## Selection Rules

The first implementation is deterministic:

- filter by valid time and transaction time
- include agent memories known to the agent even when their world-valid time is
  not the same as the queried world state
- enforce permission labels before returning atoms or evidence
- exclude simulation and hypothesis atoms from factual model context
- rank by task/goal relevance, agent memory relevance, dispute status, and
  confidence
- stop at the token budget and record budget-driven omissions as missing
  information

## Safety Rules

High-risk context preserves contradictions instead of flattening them. If a
high-risk task has unresolved contradictions, the compiler emits missing
information and recommends contradiction review.

Permission-filtered atoms are never returned to the model. Instead, the compiler
records that authorized evidence may be needed.

Simulation-shaped work is routed to recommended actions. Simulation output must
not be labeled as fact.

## Relationship To Query VM

The model context compiler sits above the native Reality Query VM. The VM finds
and reasons over atoms; the compiler packages model-ready context with memory,
belief state, missing information, assumptions, and next retrieval/tool action.

This is the AI-native interface:

```text
graph truth + belief + provenance + permissions + budget + risk
  -> compiled model context
  -> grounded model action
```
