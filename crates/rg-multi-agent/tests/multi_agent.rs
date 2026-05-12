use rg_belief::Claim;
use rg_core::{
    Confidence, EntityId, GraphValue, MemoryId, PredicateId, SourceId, TimeInterval, TxTime,
    ValidTime,
};
use rg_multi_agent::{
    AgentId, BeliefNamespace, InterAgentEvidenceExchange, MemorySharePolicy,
    MultiAgentConflictResolver, MultiAgentError, MultiAgentReality, TaskContext, TeamId,
};

#[test]
fn agent_team_and_global_belief_queries_preserve_namespaces() {
    let alpha = AgentId::new("agent-alpha");
    let beta = AgentId::new("agent-beta");
    let team = TeamId::new("team-research");
    let mut reality = MultiAgentReality::new();
    reality.register_team(team.clone(), vec![alpha.clone(), beta.clone()]);

    let private = reality
        .record_private_memory(
            alpha.clone(),
            MemoryId::new("mem-alpha-risk"),
            claim(
                "claim-alpha-risk",
                "company-a",
                "RISK",
                "supplier fragile",
                0.91,
                "src-a",
            ),
            MemorySharePolicy::private(),
        )
        .unwrap();
    let team_belief = reality
        .record_team_memory(
            team.clone(),
            alpha.clone(),
            MemoryId::new("mem-team-risk"),
            claim(
                "claim-team-risk",
                "company-a",
                "RISK",
                "team watchlist",
                0.82,
                "src-team",
            ),
            MemorySharePolicy::team(team.clone()),
        )
        .unwrap();
    let public = reality
        .record_public_world_state(
            alpha.clone(),
            MemoryId::new("mem-public-risk"),
            claim(
                "claim-public-risk",
                "company-a",
                "STATUS",
                "active",
                0.99,
                "src-public",
            ),
            MemorySharePolicy::public(),
        )
        .unwrap();

    let alpha_state = reality.agent_beliefs(&alpha).unwrap();
    assert!(claim_ids(&alpha_state.beliefs).contains(&private.claim.id));
    assert!(claim_ids(&alpha_state.beliefs).contains(&team_belief.claim.id));
    assert!(claim_ids(&alpha_state.beliefs).contains(&public.claim.id));
    assert_eq!(alpha_state.private_memory.owner_agent_id, alpha);

    let beta_state = reality.agent_beliefs(&beta).unwrap();
    assert!(!claim_ids(&beta_state.beliefs).contains(&private.claim.id));
    assert!(claim_ids(&beta_state.beliefs).contains(&team_belief.claim.id));
    assert!(claim_ids(&beta_state.beliefs).contains(&public.claim.id));

    let team_state = reality.team_beliefs(&team).unwrap();
    assert!(claim_ids(&team_state.beliefs).contains(&team_belief.claim.id));
    assert!(claim_ids(&team_state.beliefs).contains(&public.claim.id));
    assert!(!claim_ids(&team_state.beliefs).contains(&private.claim.id));

    let global = reality.globally_accepted();
    assert_eq!(claim_ids(&global.beliefs), vec![public.claim.id.clone()]);
}

#[test]
fn permissioned_evidence_exchange_updates_visibility_and_audit_trail() {
    let alpha = AgentId::new("agent-alpha");
    let beta = AgentId::new("agent-beta");
    let gamma = AgentId::new("agent-gamma");
    let mut reality = MultiAgentReality::new();
    reality.register_agent(alpha.clone());
    reality.register_agent(beta.clone());
    reality.register_agent(gamma.clone());

    let record = reality
        .record_private_memory(
            alpha.clone(),
            MemoryId::new("mem-contract"),
            claim(
                "claim-contract",
                "contract-7",
                "STATUS",
                "blocked",
                0.88,
                "src-contract",
            ),
            MemorySharePolicy::direct(vec![beta.clone()]),
        )
        .unwrap();

    assert!(!claim_ids(&reality.agent_beliefs(&beta).unwrap().beliefs).contains(&record.claim.id));

    let receipt = reality
        .exchange_evidence(InterAgentEvidenceExchange::new(
            "exchange-1",
            alpha.clone(),
            beta.clone(),
            vec![record.claim.id.clone()],
            "beta needs the blocked contract evidence for triage",
        ))
        .unwrap();

    assert!(receipt.permitted);
    assert!(claim_ids(&reality.agent_beliefs(&beta).unwrap().beliefs).contains(&record.claim.id));
    assert_eq!(reality.audit_log().last().unwrap().actor_agent_id, alpha);
    assert_eq!(
        reality.audit_log().last().unwrap().target_agent_id,
        Some(beta.clone())
    );

    let denied = reality.exchange_evidence(InterAgentEvidenceExchange::new(
        "exchange-2",
        beta,
        gamma,
        vec![record.claim.id.clone()],
        "gamma asks for a memory beta cannot reshare",
    ));
    assert!(matches!(denied, Err(MultiAgentError::ShareDenied { .. })));
}

#[test]
fn knowledge_delta_and_task_share_recommendations_are_policy_aware() {
    let alpha = AgentId::new("agent-alpha");
    let beta = AgentId::new("agent-beta");
    let team = TeamId::new("team-ops");
    let mut reality = MultiAgentReality::new();
    reality.register_team(team, vec![alpha.clone(), beta.clone()]);

    let relevant = reality
        .record_private_memory(
            alpha.clone(),
            MemoryId::new("mem-supplier-delay"),
            claim(
                "claim-supplier-delay",
                "supplier-1",
                "DELIVERY_RISK",
                "delayed contract shipment",
                0.9,
                "src-supplier",
            ),
            MemorySharePolicy::direct(vec![beta.clone()]),
        )
        .unwrap();
    let irrelevant = reality
        .record_private_memory(
            alpha.clone(),
            MemoryId::new("mem-lunch"),
            claim(
                "claim-lunch",
                "agent-alpha",
                "PREFERENCE",
                "likes tea",
                0.7,
                "src-tea",
            ),
            MemorySharePolicy::direct(vec![beta.clone()]),
        )
        .unwrap();

    let delta = reality.knowledge_delta(&alpha, &beta).unwrap();
    assert!(claim_ids(&delta.only_known_by_left).contains(&relevant.claim.id));
    assert!(claim_ids(&delta.only_known_by_left).contains(&irrelevant.claim.id));

    let recommendations = reality
        .recommend_sharing_for_task(&TaskContext::new(
            "task-supplier-recovery",
            beta.clone(),
            "recover delayed supplier contract shipment",
            vec![alpha.clone()],
        ))
        .unwrap();

    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].claim_id, relevant.claim.id);
    assert_eq!(recommendations[0].share_from, alpha);
    assert_eq!(recommendations[0].share_to, beta);
    assert!(recommendations[0].reason.contains("source-backed"));
}

#[test]
fn conflicting_agent_beliefs_are_preserved_and_resolved_without_global_collapse() {
    let alpha = AgentId::new("agent-alpha");
    let beta = AgentId::new("agent-beta");
    let mut reality = MultiAgentReality::new();
    reality.register_agent(alpha.clone());
    reality.register_agent(beta.clone());

    let alpha_claim = reality
        .record_private_memory(
            alpha.clone(),
            MemoryId::new("mem-approval"),
            claim(
                "claim-approval",
                "deal-9",
                "STATUS",
                "approved",
                0.95,
                "src-board",
            ),
            MemorySharePolicy::private(),
        )
        .unwrap();
    let beta_claim = reality
        .record_private_memory(
            beta.clone(),
            MemoryId::new("mem-blocked"),
            claim(
                "claim-blocked",
                "deal-9",
                "STATUS",
                "blocked",
                0.55,
                "src-rumor",
            ),
            MemorySharePolicy::private(),
        )
        .unwrap();

    let conflicts = reality.conflicts_for_agents(&[alpha.clone(), beta.clone()]);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].left.claim.id, alpha_claim.claim.id);
    assert_eq!(conflicts[0].right.claim.id, beta_claim.claim.id);
    assert_eq!(
        conflicts[0].left.namespace,
        BeliefNamespace::AgentPrivate(alpha.clone())
    );
    assert_eq!(
        conflicts[0].right.namespace,
        BeliefNamespace::AgentPrivate(beta.clone())
    );

    let resolution = MultiAgentConflictResolver::default().resolve(&conflicts[0]);
    assert_eq!(
        resolution.preferred_claim_id,
        Some(alpha_claim.claim.id.clone())
    );
    assert_eq!(
        resolution.preserved_claim_ids,
        vec![alpha_claim.claim.id.clone(), beta_claim.claim.id.clone()]
    );
    assert!(resolution
        .explanation
        .contains("preserved as competing beliefs"));

    assert!(
        claim_ids(&reality.agent_beliefs(&alpha).unwrap().beliefs).contains(&alpha_claim.claim.id)
    );
    assert!(
        claim_ids(&reality.agent_beliefs(&beta).unwrap().beliefs).contains(&beta_claim.claim.id)
    );
    assert!(reality.globally_accepted().beliefs.is_empty());
}

fn claim(
    id: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    confidence: f32,
    source: &str,
) -> Claim {
    Claim {
        id: rg_belief::ClaimId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Text(object.to_owned()),
        valid_time: TimeInterval::new(ValidTime::new(0), None).unwrap(),
        transaction_time: TxTime::new(1),
        confidence: Confidence::new(confidence).unwrap(),
        source_ids: vec![SourceId::new(source)],
        evidence: format!("evidence for {id}"),
    }
}

fn claim_ids(records: &[rg_multi_agent::BeliefRecord]) -> Vec<rg_belief::ClaimId> {
    records
        .iter()
        .map(|record| record.claim.id.clone())
        .collect::<Vec<_>>()
}
