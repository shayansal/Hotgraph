use rg_active_knowledge::{
    AcquisitionPlan, ClarifyingQuestionGenerator, KnowledgeAcquisitionRequest, KnowledgeGapKind,
    MissingInformationDetector, RequiredFact, RiskLevel, StalenessDetector, ToolDescriptor,
    ToolRecommendationEngine, UncertaintyEstimator, Urgency,
};
use rg_belief::Claim;
use rg_core::{
    Confidence, EntityId, GraphValue, PredicateId, SourceId, TimeInterval, TxTime, ValidTime,
};

#[test]
fn acquisition_plan_finds_missing_contract_facts_stale_evidence_and_tools() {
    let request = contract_request();

    let plan = AcquisitionPlan::build(&request);

    assert_eq!(plan.task_id, "send-contract");
    assert_eq!(plan.missing_facts.len(), 4);
    assert!(gap_kinds(&plan).contains(&KnowledgeGapKind::MissingFact));
    assert!(gap_kinds(&plan).contains(&KnowledgeGapKind::MissingSource));
    assert!(gap_kinds(&plan).contains(&KnowledgeGapKind::StaleEvidence));

    let approval = plan
        .missing_facts
        .iter()
        .find(|gap| gap.predicate.as_str() == "LATEST_APPROVAL_STATUS")
        .unwrap();
    assert_eq!(approval.urgency, Urgency::Immediate);
    assert_eq!(approval.risk_if_ignored.level, RiskLevel::Critical);
    assert_eq!(
        approval.suggested_tool.as_deref(),
        Some("fetch_contract_system_status")
    );
    assert!(approval
        .why_it_matters
        .contains("cannot be safely answered"));

    let stale = plan
        .missing_facts
        .iter()
        .find(|gap| gap.kind == KnowledgeGapKind::StaleEvidence)
        .unwrap();
    assert_eq!(stale.stale_by, Some(45));
    assert!(stale.why_it_matters.contains("45 days stale"));

    assert!(plan
        .recommended_tools
        .iter()
        .any(|tool| tool.tool_name == "fetch_contract_system_status"));
    assert!(plan
        .recommended_tools
        .iter()
        .any(|tool| tool.tool_name == "fetch_legal_review"));
    assert!(plan
        .clarifying_questions
        .iter()
        .any(|question| question.question.contains("latest approval status")));
    assert!(plan.summary.contains("4 knowledge gaps"));
}

#[test]
fn missing_information_detector_distinguishes_missing_edges_sources_and_unknown_entities() {
    let mut request = contract_request();
    request.required_facts.push(
        RequiredFact::new(
            "counterparty",
            "ULTIMATE_PARENT",
            "the counterparty parent entity must be resolved before sanctions checks",
        )
        .requires_entity_resolution()
        .with_tool("resolve_counterparty_entity"),
    );

    let gaps = MissingInformationDetector::default().detect(&request);

    assert!(gaps
        .iter()
        .any(|gap| gap.kind == KnowledgeGapKind::MissingEdge
            && gap.predicate.as_str() == "ULTIMATE_PARENT"));
    assert!(gaps
        .iter()
        .any(|gap| gap.kind == KnowledgeGapKind::UnknownEntity
            && gap.suggested_tool.as_deref() == Some("resolve_counterparty_entity")));
    assert!(gaps
        .iter()
        .any(|gap| gap.kind == KnowledgeGapKind::MissingSource
            && gap.predicate.as_str() == "LEGAL_REVIEW_STATUS"));
}

#[test]
fn staleness_and_uncertainty_are_scored_without_treating_similarity_as_truth() {
    let request = contract_request();

    let stale = StalenessDetector::new(30).detect(&request);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].stale_by, Some(45));

    let uncertainty = UncertaintyEstimator::default().estimate(&request);
    assert!(uncertainty
        .gaps
        .iter()
        .any(|gap| gap.kind == KnowledgeGapKind::UncertainEvidence));
    assert!(uncertainty.score > 0.5);
    assert!(uncertainty.explanation.contains("low-confidence"));
    assert!(uncertainty.explanation.contains("competing claims"));
}

#[test]
fn clarifying_questions_and_tool_recommendations_are_ranked_by_urgency_and_risk() {
    let request = contract_request();
    let gaps = AcquisitionPlan::build(&request).missing_facts;

    let questions = ClarifyingQuestionGenerator::default().generate(&gaps);
    assert_eq!(questions[0].urgency, Urgency::Immediate);
    assert!(questions[0].question.contains("latest approval status"));
    assert!(questions[0].why_it_matters.contains("safely answered"));

    let tools = ToolRecommendationEngine::default().recommend(&gaps, &request.available_tools);
    assert_eq!(tools[0].tool_name, "fetch_contract_system_status");
    assert_eq!(tools[0].urgency, Urgency::Immediate);
    assert!(tools[0].risk_if_skipped.contains("Critical"));
    assert!(tools.iter().all(|tool| tool
        .supporting_gap_ids
        .iter()
        .all(|id| id.starts_with("gap-"))));
}

fn contract_request() -> KnowledgeAcquisitionRequest {
    KnowledgeAcquisitionRequest {
        task_id: "send-contract".to_owned(),
        question: "Can I send this contract?".to_owned(),
        focus_entity: EntityId::new("contract-123"),
        now: TxTime::new(100),
        stale_after: 30,
        required_facts: vec![
            RequiredFact::new(
                "contract-123",
                "LATEST_APPROVAL_STATUS",
                "sending without the latest approval can violate policy",
            )
            .with_tool("fetch_contract_system_status")
            .with_source("contract system"),
            RequiredFact::new(
                "counterparty",
                "COUNTERPARTY_JURISDICTION",
                "jurisdiction controls sanctions, privacy, and signing rules",
            )
            .with_tool("lookup_counterparty_profile")
            .with_source("counterparty registry"),
            RequiredFact::new(
                "contract-123",
                "LEGAL_REVIEW_STATUS",
                "legal review is required before sending customer contracts",
            )
            .with_tool("fetch_legal_review")
            .with_source("legal review system"),
        ],
        known_claims: vec![
            claim(
                "claim-old-approval",
                "contract-123",
                "LATEST_APPROVAL_STATUS",
                "approved",
                0.93,
                vec![SourceId::new("contract-system-55-days-ago")],
                55,
            ),
            claim(
                "claim-legal-unsourced",
                "contract-123",
                "LEGAL_REVIEW_STATUS",
                "approved",
                0.91,
                Vec::new(),
                98,
            ),
            claim(
                "claim-counterparty-us",
                "counterparty",
                "COUNTERPARTY_JURISDICTION",
                "US",
                0.52,
                vec![SourceId::new("sales-note")],
                96,
            ),
            claim(
                "claim-counterparty-de",
                "counterparty",
                "COUNTERPARTY_JURISDICTION",
                "DE",
                0.5,
                vec![SourceId::new("email-thread")],
                97,
            ),
        ],
        available_tools: vec![
            ToolDescriptor::new(
                "fetch_contract_system_status",
                "contract system",
                vec!["LATEST_APPROVAL_STATUS"],
            ),
            ToolDescriptor::new(
                "lookup_counterparty_profile",
                "counterparty registry",
                vec!["COUNTERPARTY_JURISDICTION", "ULTIMATE_PARENT"],
            ),
            ToolDescriptor::new(
                "fetch_legal_review",
                "legal review system",
                vec!["LEGAL_REVIEW_STATUS"],
            ),
        ],
    }
}

fn claim(
    id: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    confidence: f32,
    source_ids: Vec<SourceId>,
    tx_time: i64,
) -> Claim {
    Claim {
        id: rg_belief::ClaimId::new(id),
        subject: EntityId::new(subject),
        predicate: PredicateId::new(predicate),
        object: GraphValue::Text(object.to_owned()),
        valid_time: TimeInterval::new(ValidTime::new(0), None).unwrap(),
        transaction_time: TxTime::new(tx_time),
        confidence: Confidence::new(confidence).unwrap(),
        source_ids,
        evidence: format!("evidence for {id}"),
    }
}

fn gap_kinds(plan: &AcquisitionPlan) -> Vec<KnowledgeGapKind> {
    plan.missing_facts
        .iter()
        .map(|gap| gap.kind)
        .collect::<Vec<_>>()
}
