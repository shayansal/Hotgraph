//! Active knowledge acquisition for Reality Graph.

use std::collections::{BTreeMap, BTreeSet};

use rg_belief::{Claim, ClaimId};
use rg_core::{EntityId, GraphValue, PredicateId, TxTime};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum KnowledgeGapKind {
    MissingFact,
    MissingEdge,
    MissingSource,
    UnknownEntity,
    StaleEvidence,
    UncertainEvidence,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Urgency {
    Low,
    Medium,
    High,
    Immediate,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskIfIgnored {
    pub level: RiskLevel,
    pub explanation: String,
}

impl RiskIfIgnored {
    fn for_gap(kind: KnowledgeGapKind, fact: &RequiredFact) -> Self {
        let predicate = fact.predicate.as_str().to_ascii_uppercase();
        let level = if predicate.contains("APPROVAL") || predicate.contains("LEGAL") {
            RiskLevel::Critical
        } else if kind == KnowledgeGapKind::UncertainEvidence
            || kind == KnowledgeGapKind::StaleEvidence
            || predicate.contains("JURISDICTION")
            || predicate.contains("PARENT")
        {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        };
        Self {
            level,
            explanation: format!("{level:?} risk if ignored: {}", fact.why_it_matters),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequiredFact {
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub why_it_matters: String,
    pub suggested_source: Option<String>,
    pub suggested_tool: Option<String>,
    pub requires_entity_resolution: bool,
}

impl RequiredFact {
    pub fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        why_it_matters: impl Into<String>,
    ) -> Self {
        Self {
            subject: EntityId::new(subject),
            predicate: PredicateId::new(predicate),
            why_it_matters: why_it_matters.into(),
            suggested_source: None,
            suggested_tool: None,
            requires_entity_resolution: false,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.suggested_source = Some(source.into());
        self
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.suggested_tool = Some(tool.into());
        self
    }

    pub fn requires_entity_resolution(mut self) -> Self {
        self.requires_entity_resolution = true;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolDescriptor {
    pub tool_name: String,
    pub source_name: String,
    pub provides_predicates: Vec<PredicateId>,
}

impl ToolDescriptor {
    pub fn new(
        tool_name: impl Into<String>,
        source_name: impl Into<String>,
        provides_predicates: Vec<&str>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            source_name: source_name.into(),
            provides_predicates: provides_predicates
                .into_iter()
                .map(PredicateId::new)
                .collect(),
        }
    }

    fn supports(&self, predicate: &PredicateId) -> bool {
        self.provides_predicates.contains(predicate)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KnowledgeAcquisitionRequest {
    pub task_id: String,
    pub question: String,
    pub focus_entity: EntityId,
    pub now: TxTime,
    pub stale_after: i64,
    pub required_facts: Vec<RequiredFact>,
    pub known_claims: Vec<Claim>,
    pub available_tools: Vec<ToolDescriptor>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KnowledgeGap {
    pub id: String,
    pub kind: KnowledgeGapKind,
    pub subject: EntityId,
    pub predicate: PredicateId,
    pub related_claim_ids: Vec<ClaimId>,
    pub why_it_matters: String,
    pub suggested_source: Option<String>,
    pub suggested_tool: Option<String>,
    pub urgency: Urgency,
    pub risk_if_ignored: RiskIfIgnored,
    pub stale_by: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClarifyingQuestion {
    pub gap_id: String,
    pub question: String,
    pub why_it_matters: String,
    pub urgency: Urgency,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRecommendation {
    pub tool_name: String,
    pub source_name: String,
    pub reason: String,
    pub urgency: Urgency,
    pub risk_if_skipped: String,
    pub supporting_gap_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UncertaintyReport {
    pub score: f32,
    pub gaps: Vec<KnowledgeGap>,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcquisitionPlan {
    pub task_id: String,
    pub question: String,
    pub missing_facts: Vec<KnowledgeGap>,
    pub clarifying_questions: Vec<ClarifyingQuestion>,
    pub recommended_tools: Vec<ToolRecommendation>,
    pub urgency: Urgency,
    pub risk_if_ignored: RiskIfIgnored,
    pub summary: String,
}

impl AcquisitionPlan {
    pub fn build(request: &KnowledgeAcquisitionRequest) -> Self {
        let missing_detector = MissingInformationDetector::default();
        let staleness_detector = StalenessDetector::new(request.stale_after);
        let uncertainty_estimator = UncertaintyEstimator::default();

        let mut gaps = missing_detector.detect(request);
        gaps.extend(staleness_detector.detect(request));
        gaps.extend(uncertainty_estimator.estimate(request).gaps);
        dedupe_and_sort_gaps(&mut gaps);

        let clarifying_questions = ClarifyingQuestionGenerator::default().generate(&gaps);
        let recommended_tools =
            ToolRecommendationEngine::default().recommend(&gaps, &request.available_tools);
        let urgency = gaps
            .iter()
            .map(|gap| gap.urgency)
            .max()
            .unwrap_or(Urgency::Low);
        let risk_if_ignored = gaps
            .iter()
            .max_by_key(|gap| gap.risk_if_ignored.level)
            .map(|gap| gap.risk_if_ignored.clone())
            .unwrap_or_else(|| RiskIfIgnored {
                level: RiskLevel::Low,
                explanation: "no material knowledge gap detected".to_owned(),
            });
        let summary = format!(
            "{} knowledge gaps found for task {}; highest urgency {:?}, highest risk {:?}",
            gaps.len(),
            request.task_id,
            urgency,
            risk_if_ignored.level
        );

        Self {
            task_id: request.task_id.clone(),
            question: request.question.clone(),
            missing_facts: gaps,
            clarifying_questions,
            recommended_tools,
            urgency,
            risk_if_ignored,
            summary,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MissingInformationDetector {
    pub treat_stale_latest_as_missing: bool,
}

impl Default for MissingInformationDetector {
    fn default() -> Self {
        Self {
            treat_stale_latest_as_missing: true,
        }
    }
}

impl MissingInformationDetector {
    pub fn detect(&self, request: &KnowledgeAcquisitionRequest) -> Vec<KnowledgeGap> {
        let mut gaps = Vec::new();
        for fact in &request.required_facts {
            let claims = matching_claims(request, fact);
            if claims.is_empty() {
                let kind = if looks_like_edge(&fact.predicate) {
                    KnowledgeGapKind::MissingEdge
                } else {
                    KnowledgeGapKind::MissingFact
                };
                gaps.push(gap_from_fact(
                    kind,
                    fact,
                    Vec::new(),
                    format!(
                        "{} cannot be safely answered without {} for {}",
                        request.question,
                        human_predicate(&fact.predicate),
                        fact.subject
                    ),
                    None,
                ));
            }

            if self.treat_stale_latest_as_missing
                && !claims.is_empty()
                && fact
                    .predicate
                    .as_str()
                    .to_ascii_uppercase()
                    .starts_with("LATEST_")
                && claims.iter().all(|claim| {
                    request.now.as_i64() - claim.transaction_time.as_i64() > request.stale_after
                })
            {
                gaps.push(gap_from_fact(
                    KnowledgeGapKind::MissingFact,
                    fact,
                    claims.iter().map(|claim| claim.id.clone()).collect(),
                    format!(
                        "{} cannot be safely answered because the latest {} is missing; existing evidence is stale",
                        request.question,
                        human_predicate(&fact.predicate)
                    ),
                    None,
                ));
            }

            if fact.requires_entity_resolution {
                gaps.push(gap_from_fact(
                    KnowledgeGapKind::UnknownEntity,
                    fact,
                    claims.iter().map(|claim| claim.id.clone()).collect(),
                    format!(
                        "unknown entity resolution blocks reliable use of {}",
                        human_predicate(&fact.predicate)
                    ),
                    None,
                ));
            }

            for claim in claims {
                if claim.source_ids.is_empty() {
                    gaps.push(gap_from_fact(
                        KnowledgeGapKind::MissingSource,
                        fact,
                        vec![claim.id.clone()],
                        format!(
                            "{} is asserted but has no provenance source; LLMs may summarize only evidence the graph can name",
                            human_predicate(&fact.predicate)
                        ),
                        None,
                    ));
                }
            }
        }
        gaps
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StalenessDetector {
    stale_after: i64,
}

impl StalenessDetector {
    pub fn new(stale_after: i64) -> Self {
        Self { stale_after }
    }

    pub fn detect(&self, request: &KnowledgeAcquisitionRequest) -> Vec<KnowledgeGap> {
        let mut gaps = Vec::new();
        for fact in &request.required_facts {
            for claim in matching_claims(request, fact) {
                let age = request.now.as_i64() - claim.transaction_time.as_i64();
                if age > self.stale_after {
                    let stale_by = age;
                    gaps.push(gap_from_fact(
                        KnowledgeGapKind::StaleEvidence,
                        fact,
                        vec![claim.id.clone()],
                        format!(
                            "{} evidence is {stale_by} days stale; refresh before acting on {}",
                            human_predicate(&fact.predicate),
                            request.question
                        ),
                        Some(stale_by),
                    ));
                }
            }
        }
        gaps
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UncertaintyEstimator {
    low_confidence_threshold: f32,
}

impl Default for UncertaintyEstimator {
    fn default() -> Self {
        Self {
            low_confidence_threshold: 0.7,
        }
    }
}

impl UncertaintyEstimator {
    pub fn estimate(&self, request: &KnowledgeAcquisitionRequest) -> UncertaintyReport {
        let mut gaps = Vec::new();
        let mut explanations = Vec::new();
        for fact in &request.required_facts {
            let claims = matching_claims(request, fact);
            let low_confidence = claims
                .iter()
                .filter(|claim| claim.confidence.as_f32() < self.low_confidence_threshold)
                .collect::<Vec<_>>();
            if !low_confidence.is_empty() {
                explanations.push(format!(
                    "low-confidence evidence for {}",
                    human_predicate(&fact.predicate)
                ));
                gaps.push(gap_from_fact(
                    KnowledgeGapKind::UncertainEvidence,
                    fact,
                    low_confidence
                        .iter()
                        .map(|claim| claim.id.clone())
                        .collect(),
                    format!(
                        "{} has low-confidence evidence and needs corroboration",
                        human_predicate(&fact.predicate)
                    ),
                    None,
                ));
            }

            if has_competing_objects(&claims) {
                explanations.push(format!(
                    "competing claims for {}",
                    human_predicate(&fact.predicate)
                ));
                gaps.push(gap_from_fact(
                    KnowledgeGapKind::UncertainEvidence,
                    fact,
                    claims.iter().map(|claim| claim.id.clone()).collect(),
                    format!(
                        "{} has competing claims; retrieve both sides before acting",
                        human_predicate(&fact.predicate)
                    ),
                    None,
                ));
            }
        }
        dedupe_and_sort_gaps(&mut gaps);

        let score = if request.required_facts.is_empty() {
            0.0
        } else {
            (explanations.len() as f32 / request.required_facts.len() as f32).min(1.0)
        };
        let explanation = if explanations.is_empty() {
            "no material uncertainty detected".to_owned()
        } else {
            explanations.sort();
            explanations.dedup();
            explanations.join("; ")
        };

        UncertaintyReport {
            score,
            gaps,
            explanation,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClarifyingQuestionGenerator {
    pub max_questions: Option<usize>,
}

impl ClarifyingQuestionGenerator {
    pub fn generate(&self, gaps: &[KnowledgeGap]) -> Vec<ClarifyingQuestion> {
        let mut questions = gaps
            .iter()
            .map(|gap| ClarifyingQuestion {
                gap_id: gap.id.clone(),
                question: question_for_gap(gap),
                why_it_matters: gap.why_it_matters.clone(),
                urgency: gap.urgency,
            })
            .collect::<Vec<_>>();
        questions.sort_by(|left, right| {
            right
                .urgency
                .cmp(&left.urgency)
                .then_with(|| gap_sort_rank(&left.gap_id).cmp(&gap_sort_rank(&right.gap_id)))
                .then_with(|| left.gap_id.cmp(&right.gap_id))
        });
        if let Some(max_questions) = self.max_questions {
            questions.truncate(max_questions);
        }
        questions
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolRecommendationEngine {
    pub prefer_explicit_tools: bool,
}

impl Default for ToolRecommendationEngine {
    fn default() -> Self {
        Self {
            prefer_explicit_tools: true,
        }
    }
}

impl ToolRecommendationEngine {
    pub fn recommend(
        &self,
        gaps: &[KnowledgeGap],
        available_tools: &[ToolDescriptor],
    ) -> Vec<ToolRecommendation> {
        let mut by_tool = BTreeMap::<String, ToolRecommendation>::new();
        for gap in gaps {
            let explicit_tool = gap
                .suggested_tool
                .as_ref()
                .and_then(|name| available_tools.iter().find(|tool| &tool.tool_name == name));
            let predicate_tool = || {
                available_tools
                    .iter()
                    .find(|tool| tool.supports(&gap.predicate))
            };
            let tool = if self.prefer_explicit_tools {
                explicit_tool.or_else(predicate_tool)
            } else {
                predicate_tool().or(explicit_tool)
            };
            let Some(tool) = tool else {
                continue;
            };
            let entry =
                by_tool
                    .entry(tool.tool_name.clone())
                    .or_insert_with(|| ToolRecommendation {
                        tool_name: tool.tool_name.clone(),
                        source_name: tool.source_name.clone(),
                        reason: format!(
                            "fetch {} from {} to close active knowledge gaps",
                            human_predicate(&gap.predicate),
                            tool.source_name
                        ),
                        urgency: gap.urgency,
                        risk_if_skipped: format!(
                            "{:?}: {}",
                            gap.risk_if_ignored.level, gap.risk_if_ignored.explanation
                        ),
                        supporting_gap_ids: Vec::new(),
                    });
            entry.urgency = entry.urgency.max(gap.urgency);
            if gap.risk_if_ignored.level > risk_level_from_text(&entry.risk_if_skipped) {
                entry.risk_if_skipped = format!(
                    "{:?}: {}",
                    gap.risk_if_ignored.level, gap.risk_if_ignored.explanation
                );
            }
            entry.supporting_gap_ids.push(gap.id.clone());
            entry.supporting_gap_ids.sort();
            entry.supporting_gap_ids.dedup();
        }

        let mut recommendations = by_tool.into_values().collect::<Vec<_>>();
        recommendations.sort_by(|left, right| {
            right
                .urgency
                .cmp(&left.urgency)
                .then_with(|| left.tool_name.cmp(&right.tool_name))
        });
        recommendations
    }
}

fn matching_claims<'a>(
    request: &'a KnowledgeAcquisitionRequest,
    fact: &RequiredFact,
) -> Vec<&'a Claim> {
    request
        .known_claims
        .iter()
        .filter(|claim| claim.subject == fact.subject && claim.predicate == fact.predicate)
        .collect()
}

fn gap_from_fact(
    kind: KnowledgeGapKind,
    fact: &RequiredFact,
    related_claim_ids: Vec<ClaimId>,
    why_it_matters: String,
    stale_by: Option<i64>,
) -> KnowledgeGap {
    KnowledgeGap {
        id: gap_id(kind, fact, &related_claim_ids),
        kind,
        subject: fact.subject.clone(),
        predicate: fact.predicate.clone(),
        related_claim_ids,
        why_it_matters,
        suggested_source: fact.suggested_source.clone(),
        suggested_tool: fact.suggested_tool.clone(),
        urgency: urgency_for(kind, fact),
        risk_if_ignored: RiskIfIgnored::for_gap(kind, fact),
        stale_by,
    }
}

fn gap_id(kind: KnowledgeGapKind, fact: &RequiredFact, claim_ids: &[ClaimId]) -> String {
    let claim_suffix = if claim_ids.is_empty() {
        "none".to_owned()
    } else {
        claim_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("-")
    };
    format!(
        "gap-{:?}-{}-{}-{}",
        kind, fact.subject, fact.predicate, claim_suffix
    )
    .to_ascii_lowercase()
}

fn urgency_for(kind: KnowledgeGapKind, fact: &RequiredFact) -> Urgency {
    let predicate = fact.predicate.as_str().to_ascii_uppercase();
    if predicate.contains("APPROVAL") || predicate.contains("LEGAL") {
        Urgency::Immediate
    } else if kind == KnowledgeGapKind::MissingFact
        || kind == KnowledgeGapKind::MissingEdge
        || predicate.contains("JURISDICTION")
        || predicate.contains("PARENT")
    {
        Urgency::High
    } else if kind == KnowledgeGapKind::StaleEvidence || kind == KnowledgeGapKind::UncertainEvidence
    {
        Urgency::Medium
    } else {
        Urgency::Low
    }
}

fn looks_like_edge(predicate: &PredicateId) -> bool {
    let predicate = predicate.as_str().to_ascii_uppercase();
    predicate.contains("PARENT")
        || predicate.contains("OWNER")
        || predicate.contains("COUNTERPARTY")
        || predicate.contains("SUPPLIER")
        || predicate.contains("RELATIONSHIP")
}

fn has_competing_objects(claims: &[&Claim]) -> bool {
    let objects = claims
        .iter()
        .map(|claim| graph_value_key(&claim.object))
        .collect::<BTreeSet<_>>();
    objects.len() > 1
}

fn graph_value_key(value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(entity_id) => format!("entity:{entity_id}"),
        GraphValue::Text(value) => format!("text:{}", value.to_ascii_lowercase()),
        GraphValue::Integer(value) => format!("int:{value}"),
        GraphValue::Decimal(value) => format!("decimal:{value:.6}"),
        GraphValue::Boolean(value) => format!("bool:{value}"),
        GraphValue::Time(value) => format!("time:{}", value.as_i64()),
        GraphValue::Null => "null".to_owned(),
    }
}

fn question_for_gap(gap: &KnowledgeGap) -> String {
    let predicate = human_predicate(&gap.predicate);
    match gap.kind {
        KnowledgeGapKind::MissingFact | KnowledgeGapKind::MissingEdge => {
            format!("What is the {predicate} for {}?", gap.subject)
        }
        KnowledgeGapKind::MissingSource => {
            format!("Which source proves the {predicate} for {}?", gap.subject)
        }
        KnowledgeGapKind::UnknownEntity => {
            format!("Which real-world entity does {} refer to?", gap.subject)
        }
        KnowledgeGapKind::StaleEvidence => {
            format!("Can you refresh the {predicate} for {}?", gap.subject)
        }
        KnowledgeGapKind::UncertainEvidence => {
            format!("Can you corroborate the {predicate} for {}?", gap.subject)
        }
    }
}

fn human_predicate(predicate: &PredicateId) -> String {
    predicate.as_str().to_ascii_lowercase().replace('_', " ")
}

fn dedupe_and_sort_gaps(gaps: &mut Vec<KnowledgeGap>) {
    gaps.sort_by(|left, right| {
        right
            .urgency
            .cmp(&left.urgency)
            .then_with(|| right.risk_if_ignored.level.cmp(&left.risk_if_ignored.level))
            .then_with(|| gap_kind_rank(left.kind).cmp(&gap_kind_rank(right.kind)))
            .then_with(|| left.id.cmp(&right.id))
    });
    gaps.dedup_by(|left, right| left.id == right.id);
}

fn gap_sort_rank(gap_id: &str) -> u8 {
    if gap_id.contains("missingfact") {
        0
    } else if gap_id.contains("missingsource") {
        1
    } else if gap_id.contains("staleevidence") {
        2
    } else if gap_id.contains("uncertainevidence") {
        3
    } else if gap_id.contains("unknownentity") {
        4
    } else {
        5
    }
}

fn gap_kind_rank(kind: KnowledgeGapKind) -> u8 {
    match kind {
        KnowledgeGapKind::MissingFact => 0,
        KnowledgeGapKind::MissingEdge => 1,
        KnowledgeGapKind::MissingSource => 2,
        KnowledgeGapKind::StaleEvidence => 3,
        KnowledgeGapKind::UncertainEvidence => 4,
        KnowledgeGapKind::UnknownEntity => 5,
    }
}

fn risk_level_from_text(text: &str) -> RiskLevel {
    if text.starts_with("Critical") {
        RiskLevel::Critical
    } else if text.starts_with("High") {
        RiskLevel::High
    } else if text.starts_with("Medium") {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}
