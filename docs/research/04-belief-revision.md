# Belief Revision for LLM Agents Using Source-Backed Temporal Graphs

## Abstract

LLM agents often overwrite memory when corrected, losing the ability to explain
what they believed before and why belief changed. Reality Graph models belief as
a source-backed temporal graph process: claims coexist, conflicts are grouped,
resolution policies choose current belief, and transaction time records belief
history. This paper studies whether explicit belief revision improves agent
honesty, contradiction handling, and temporal self-explanation.

## Related Work

Classical belief revision studies how knowledge bases change under new
information. Knowledge graphs represent structured claims, while RAG systems
retrieve evidence for answers. Agent memory systems often store either raw
conversation or summaries. Reality Graph connects these threads by retaining
competing source-backed claims and asking agents to explain belief state under
valid-time and transaction-time constraints.

## Method

The `rg-belief` layer introduces `Claim`, `BeliefState`, `BeliefRevision`,
`ConflictSet`, `ResolutionPolicy`, and `SourceTrustModel`. Conflicts cover
mutually exclusive status, date mismatch, numeric mismatch, entity identity
mismatch, causal disagreement, source trust disagreement, and valid-time overlap.
The system never deletes losing claims; it marks preferred claims with reasons,
confidence, source trust, and transaction-time revision history.

## Datasets

Datasets contain acquisition timelines, executive roles, ownership percentages,
legal status changes, and geopolitical event claims where sources disagree.
Synthetic fixtures pair hidden ground truth with noisy source reports, rumors,
later corrections, and regulatory updates.

## Experiments

Models answer claim-verification and explanation questions with and without the
belief engine. Tasks ask for both sides of a conflict, current preferred belief,
the reason for preference, prior belief at an earlier transaction time, and what
changed after new evidence arrived. Metrics include conflict detection F1,
preferred-claim accuracy, historical belief accuracy, citation faithfulness, and
calibration of uncertainty language.

## Ablations

- Collapse conflicts into a single latest claim.
- Remove source trust.
- Remove transaction-time belief history.
- Remove valid-time overlap checks.
- Disable numeric and date mismatch classifiers.
- Return only preferred belief, not losing claims.

## Limitations

Resolution policies are domain-dependent and may encode institutional bias.
Source trust models can be wrong or gamed. Some conflicts are semantic rather
than structural and require better ontology or extraction support. Belief
revision makes agents more honest about uncertainty but does not guarantee
truth.

## Reproducibility Checklist

- Publish conflict ontology and mutually exclusive predicate rules.
- Publish source trust settings and resolution policies.
- Save every claim, conflict set, revision event, and preferred-belief decision.
- Evaluate current belief and historical belief separately.
- Include examples where the system should abstain.
- Report all unresolved conflicts, not only successful resolutions.
