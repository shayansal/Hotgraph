# Direct Model Runtime Integration

Reality Graph runtime integration is for model execution loops that need memory and belief state before, during, and after generation. Tool calls remain useful, but `rg-runtime` exposes hooks that can sit inside inference prefill, decoding, agent-loop, and final-answer guardrail stages.

## Runtime Hooks

`ModelRuntimeBridge` exposes six experimental hooks:

- `prefill_context_pack`: compile a source-backed `EvidencePack` before pre-attention/prefill.
- `refresh_context_during_agent_loop`: refresh compact context during long-running agent loops.
- `retrieve_before_tool_choice`: retrieve graph context before the model chooses an external tool.
- `verify_before_final_answer`: block final answers that lack supporting assertions.
- `write_memory_after_action`: record source-backed episodic memory after an action outcome.
- `update_belief_after_observation`: update the external belief-state cache after an observation.

Each hook returns a `RuntimePhase` and a `hook_trace` so runtime decisions can be replayed and audited.

## Open-Source Inference Servers

Reference target: vLLM, TGI, llama.cpp server, or similar local inference servers.

Recommended flow:

1. Call `prefill_context_pack` before prompt prefill.
2. Inject only the returned `prompt_prefix` and citation metadata into model-visible context.
3. During decoding or between generation segments, call `retrieve_before_tool_choice` when tool choice is available.
4. Before emitting a final answer, call `verify_before_final_answer` with the assertion ID the answer relies on.

This integration should treat Reality Graph context as a deterministic, source-backed prefix. It should not allow the model to invent citations or mutate graph state from decoded text alone.

## Local Agent Runtimes

Reference target: LangGraph-style loops, local research agents, or custom planner/executor runtimes.

Recommended flow:

1. Maintain an `AgentLoopState` with the agent ID, current task, turn number, and active entity IDs.
2. Call `refresh_context_during_agent_loop` at planning checkpoints, after tool results, and when the active entity set changes.
3. Call `write_memory_after_action` only after the action has an outcome and source IDs.
4. Call `update_belief_after_observation` when the agent observes a new claim that should affect belief state.

Agent memory writes should remain source-backed. Belief updates should preserve competing claims instead of collapsing the graph to a single naive fact.

## Research Notebooks

Reference target: deterministic experiments, evaluation notebooks, and ablation harnesses.

Recommended flow:

1. Build a fixed `InMemoryStorage` fixture.
2. Run each hook with a deterministic `RuntimeProfile`.
3. Persist the returned `hook_trace`, assertion IDs, source IDs, and belief cache keys.
4. Compare notebook outputs across model/runtime changes.

Notebook integrations are the right place to test graph-conditioned planning, memory-aware speculative decoding hints, and final-answer verification policies before wiring them into production runtime servers.

## Safety Rules

- Runtime hooks may retrieve or package evidence; they must not treat model output as fact.
- Memory writes need source IDs and a revision path.
- Final answers should carry support from assertions, sources, or an explicit insufficient-evidence guardrail.
- Simulation, belief state, and retrieval traces must be labeled as runtime support, not as direct observations.
