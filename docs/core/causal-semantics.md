# Causal Semantics

Reality Kernel keeps causal structure separate from normal graph association.

A normal relationship says two things are connected. A causal atom says one
event influenced another through a mechanism, with confidence and evidence.

## Causal Atom

```rust
pub struct CausalAtom {
    pub cause: EventId,
    pub effect: EventId,
    pub mechanism: Option<String>,
    pub lag: Option<Duration>,
    pub confidence: Confidence,
    pub evidence: Vec<SourceId>,
    pub counterfactual_notes: Vec<String>,
}
```

Rules:

- A causal atom must include evidence source IDs.
- A causal atom cannot cause itself.
- Confidence belongs to the causal claim, not to the existence of either event.
- Counterfactual notes are strategy support, not fact.

## Native Questions

The kernel supports causal questions directly:

```text
WhatCaused(event_id, max_depth)
WhatMightHappenNext(event_id, max_depth)
WhatBreaksIfEventDoesNotOccur(event_id, max_depth)
```

`WhatCaused` walks upstream causal atoms. `WhatMightHappenNext` walks downstream
causal atoms. `WhatBreaksIfEventDoesNotOccur` returns affected downstream events,
paths, risks, notes, and a warning that the output is counterfactual simulation.

## Non-Fact Rule

Causal traversal can support strategy, planning, and risk analysis, but it must
not relabel simulation as fact. Counterfactual results describe possible impact
under an intervention, not a changed reality.
