//! Synthetic world generation for Reality Graph benchmarks.

use std::collections::{BTreeMap, BTreeSet};

use rg_core::{
    Assertion, AssertionId, AssertionStatus, Confidence, ContentHash, ContextScope, Entity,
    EntityId, EntityType, EventId, GraphValue, PredicateId, PropertyMap, SourceId, TimeInterval,
    TxTime, ValidTime,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldSchema {
    pub seed: u64,
    pub company_count: usize,
    pub person_count: usize,
    pub event_count: usize,
    pub document_count: usize,
    pub contradiction_count: usize,
    pub causal_chain_count: usize,
    pub agent_task_count: usize,
    pub noise_rate: f32,
}

impl WorldSchema {
    pub fn controlled(seed: u64) -> Self {
        Self {
            seed,
            company_count: 4,
            person_count: 6,
            event_count: 8,
            document_count: 12,
            contradiction_count: 2,
            causal_chain_count: 2,
            agent_task_count: AgentTaskType::all().len(),
            noise_rate: 0.35,
        }
    }

    pub fn with_companies(mut self, company_count: usize) -> Self {
        self.company_count = company_count.max(1);
        self
    }

    pub fn with_people(mut self, person_count: usize) -> Self {
        self.person_count = person_count.max(1);
        self
    }

    pub fn with_events(mut self, event_count: usize) -> Self {
        self.event_count = event_count.max(1);
        self
    }

    pub fn with_documents(mut self, document_count: usize) -> Self {
        self.document_count = document_count.max(1);
        self
    }

    pub fn with_contradictions(mut self, contradiction_count: usize) -> Self {
        self.contradiction_count = contradiction_count;
        self
    }

    pub fn with_causal_chains(mut self, causal_chain_count: usize) -> Self {
        self.causal_chain_count = causal_chain_count;
        self
    }

    pub fn with_agent_tasks(mut self, agent_task_count: usize) -> Self {
        self.agent_task_count = agent_task_count.max(1);
        self
    }

    pub fn generate(&self) -> Result<SyntheticWorld, String> {
        let entities = EntityGenerator::generate(self);
        let events = EventGenerator::generate(self, &entities);
        let mut documents = DocumentGenerator::generate(self, &events);
        let hidden_true_state = self.hidden_truth_from(&entities, &documents)?;
        attach_assertions_to_documents(&mut documents, &hidden_true_state.assertions);
        let contradictions = ContradictionGenerator::generate(self, &hidden_true_state, &documents);
        let rumors = rumor_assertions(self, &hidden_true_state, &documents);
        let noisy_assertions = hidden_true_state
            .assertions
            .iter()
            .cloned()
            .chain(
                contradictions
                    .iter()
                    .map(|pair| pair.observed_assertion.clone()),
            )
            .chain(rumors.iter().cloned())
            .collect::<Vec<_>>();
        let causal_chains = CausalChainGenerator::generate(self, &events);
        let noisy_observed_state = NoisyObservedState {
            assertions: noisy_assertions,
            source_documents: documents.clone(),
            contradictions: contradictions.clone(),
            rumors,
        };
        let benchmark_tasks =
            AgentTaskGenerator::generate(self, &hidden_true_state, &contradictions, &causal_chains);

        Ok(SyntheticWorld {
            schema: *self,
            entities,
            events,
            documents,
            hidden_true_state,
            noisy_observed_state,
            causal_chains,
            benchmark_tasks,
        })
    }

    pub fn hidden_truth_from(
        &self,
        entities: &[Entity],
        documents: &[SourceDocument],
    ) -> Result<HiddenTrueState, String> {
        if entities.is_empty() {
            return Err("world requires entities".to_string());
        }
        if documents.is_empty() {
            return Err("world requires source documents".to_string());
        }

        let companies = entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Organization)
            .collect::<Vec<_>>();
        let people = entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Person)
            .collect::<Vec<_>>();
        if companies.is_empty() || people.is_empty() {
            return Err("world requires at least one company and one person".to_string());
        }

        let mut assertions = Vec::new();
        for index in 0..self.company_count.max(self.person_count).max(4) {
            let person = people[index % people.len()];
            let company = companies[(index + stable_offset(self.seed, index)) % companies.len()];
            let document = &documents[index % documents.len()];
            assertions.push(assertion(AssertionDraft {
                id: format!("truth-worked-at-{index:04}"),
                subject: person.id.clone(),
                predicate: "WORKED_AT",
                object: GraphValue::Entity(company.id.clone()),
                valid_from: 2020 + (index % 3) as i64,
                valid_to: Some(2025),
                confidence: 0.86,
                source_id: document.id.clone(),
            }));
        }
        for index in 0..companies.len().saturating_sub(1) {
            let document = &documents[(index + people.len()) % documents.len()];
            assertions.push(assertion(AssertionDraft {
                id: format!("truth-supplies-{index:04}"),
                subject: companies[index].id.clone(),
                predicate: "SUPPLIES",
                object: GraphValue::Entity(companies[index + 1].id.clone()),
                valid_from: 2021,
                valid_to: None,
                confidence: 0.78,
                source_id: document.id.clone(),
            }));
        }
        if let Some(company) = companies.first() {
            let document = &documents[(documents.len() - 1).min(1)];
            assertions.push(assertion(AssertionDraft {
                id: "truth-policy-0000".to_string(),
                subject: company.id.clone(),
                predicate: "POLICY_STATUS",
                object: GraphValue::Text("compliant".to_string()),
                valid_from: 2023,
                valid_to: None,
                confidence: 0.82,
                source_id: document.id.clone(),
            }));
        }

        assertions.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(HiddenTrueState { assertions })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticWorld {
    pub schema: WorldSchema,
    pub entities: Vec<Entity>,
    pub events: Vec<WorldEvent>,
    pub documents: Vec<SourceDocument>,
    pub hidden_true_state: HiddenTrueState,
    pub noisy_observed_state: NoisyObservedState,
    pub causal_chains: Vec<CausalChain>,
    pub benchmark_tasks: Vec<BenchmarkTask>,
}

impl SyntheticWorld {
    pub fn companies(&self) -> Vec<&Entity> {
        self.entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Organization)
            .collect()
    }

    pub fn people(&self) -> Vec<&Entity> {
        self.entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Person)
            .collect()
    }

    pub fn fingerprint(&self) -> String {
        let mut parts = vec![
            format!("seed={}", self.schema.seed),
            format!("entities={}", self.entities.len()),
            format!("events={}", self.events.len()),
            format!("docs={}", self.documents.len()),
        ];
        parts.extend(
            self.entities
                .iter()
                .map(|entity| format!("entity={}", entity.id)),
        );
        parts.extend(
            self.hidden_true_state
                .assertions
                .iter()
                .map(|assertion| format!("truth={}", assertion.id)),
        );
        parts.extend(
            self.noisy_observed_state
                .assertions
                .iter()
                .map(|assertion| format!("observed={}", assertion.id)),
        );
        format!("world-{:016x}", stable_hash(parts.join("|").as_bytes()))
    }
}

pub struct EntityGenerator;

impl EntityGenerator {
    pub fn generate(schema: &WorldSchema) -> Vec<Entity> {
        let mut entities = Vec::with_capacity(schema.company_count + schema.person_count);
        for index in 0..schema.company_count {
            entities.push(Entity {
                id: EntityId::new(format!("company-{index:04}")),
                entity_type: EntityType::Organization,
                canonical_name: Some(format!("Company {index}")),
                properties: PropertyMap::default(),
                created_tx: TxTime::new(1),
            });
        }
        for index in 0..schema.person_count {
            entities.push(Entity {
                id: EntityId::new(format!("person-{index:04}")),
                entity_type: EntityType::Person,
                canonical_name: Some(format!("Person {index}")),
                properties: PropertyMap::default(),
                created_tx: TxTime::new(1),
            });
        }
        entities
    }
}

pub struct EventGenerator;

impl EventGenerator {
    pub fn generate(schema: &WorldSchema, entities: &[Entity]) -> Vec<WorldEvent> {
        let kinds = [
            "meeting",
            "contract_signed",
            "news",
            "policy_change",
            "supply_disruption",
            "leadership_change",
            "rumor",
            "audit",
        ];
        let entity_ids = entities
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::with_capacity(schema.event_count);
        for index in 0..schema.event_count {
            let subject = entity_ids
                .get(index % entity_ids.len().max(1))
                .cloned()
                .unwrap_or_else(|| EntityId::new("entity-missing"));
            let object = entity_ids
                .get((index + 1) % entity_ids.len().max(1))
                .cloned();
            let kind = kinds[index % kinds.len()].to_string();
            events.push(WorldEvent {
                id: EventId::new(format!("event-{index:04}")),
                kind: kind.clone(),
                subject,
                object,
                valid_time: ValidTime::new(2020 + index as i64),
                description: format!("Synthetic {kind} event {index}"),
            });
        }
        events
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldEvent {
    pub id: EventId,
    pub kind: String,
    pub subject: EntityId,
    pub object: Option<EntityId>,
    pub valid_time: ValidTime,
    pub description: String,
}

pub struct DocumentGenerator;

impl DocumentGenerator {
    pub fn generate(schema: &WorldSchema, events: &[WorldEvent]) -> Vec<SourceDocument> {
        let kinds = [
            SourceDocumentKind::Document,
            SourceDocumentKind::Email,
            SourceDocumentKind::Contract,
            SourceDocumentKind::MeetingNote,
            SourceDocumentKind::News,
            SourceDocumentKind::PolicyChange,
            SourceDocumentKind::Rumor,
        ];
        let mut documents = Vec::with_capacity(schema.document_count);
        for index in 0..schema.document_count {
            let kind = kinds[index % kinds.len()];
            let event = &events[index % events.len().max(1)];
            let source_id = SourceId::new(format!("source-{index:04}"));
            documents.push(SourceDocument {
                id: source_id.clone(),
                kind,
                title: format!("{kind:?} {}", index),
                body: format!(
                    "{kind:?} source {} observes {} involving {}.",
                    index, event.description, event.subject
                ),
                observed_at: TxTime::new(100 + index as i64),
                trust_score: source_trust(kind),
                supported_assertion_ids: Vec::new(),
                content_hash: ContentHash::new(format!(
                    "sha256:{:016x}",
                    stable_hash(format!("{}-{kind:?}-{}", schema.seed, index).as_bytes())
                )),
            });
        }
        documents
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceDocumentKind {
    Document,
    Email,
    Contract,
    MeetingNote,
    News,
    PolicyChange,
    Rumor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceDocument {
    pub id: SourceId,
    pub kind: SourceDocumentKind,
    pub title: String,
    pub body: String,
    pub observed_at: TxTime,
    pub trust_score: f32,
    pub supported_assertion_ids: Vec<AssertionId>,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HiddenTrueState {
    pub assertions: Vec<Assertion>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoisyObservedState {
    pub assertions: Vec<Assertion>,
    pub source_documents: Vec<SourceDocument>,
    pub contradictions: Vec<ContradictionPair>,
    pub rumors: Vec<Assertion>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContradictionPair {
    pub id: String,
    pub hidden_true_assertion: AssertionId,
    pub observed_false_assertion: AssertionId,
    pub observed_assertion: Assertion,
    pub explanation: String,
}

pub struct ContradictionGenerator;

impl ContradictionGenerator {
    pub fn generate(
        schema: &WorldSchema,
        truth: &HiddenTrueState,
        documents: &[SourceDocument],
    ) -> Vec<ContradictionPair> {
        let rumor_source = documents
            .iter()
            .find(|document| document.kind == SourceDocumentKind::Rumor)
            .or_else(|| documents.first());
        let Some(source) = rumor_source else {
            return Vec::new();
        };
        truth
            .assertions
            .iter()
            .take(schema.contradiction_count)
            .enumerate()
            .map(|(index, true_assertion)| {
                let false_id = AssertionId::new(format!("observed-contradiction-{index:04}"));
                let observed_assertion = Assertion {
                    id: false_id.clone(),
                    subject: true_assertion.subject.clone(),
                    predicate: true_assertion.predicate.clone(),
                    object: contradictory_object(&true_assertion.object, index),
                    valid_time: true_assertion.valid_time.clone(),
                    transaction_time: TimeInterval::new(TxTime::new(500 + index as i64), None)
                        .expect("valid transaction interval"),
                    confidence: Confidence::new(0.41).expect("confidence"),
                    source_ids: vec![source.id.clone()],
                    context: true_assertion.context.clone(),
                    status: AssertionStatus::Active,
                };
                ContradictionPair {
                    id: format!("contradiction-{index:04}"),
                    hidden_true_assertion: true_assertion.id.clone(),
                    observed_false_assertion: false_id,
                    observed_assertion,
                    explanation: format!(
                        "Noisy source conflicts with hidden truth {}",
                        true_assertion.id
                    ),
                }
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalChain {
    pub id: String,
    pub events: Vec<EventId>,
    pub links: Vec<CausalChainLink>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CausalChainLink {
    pub cause_event: EventId,
    pub effect_event: EventId,
    pub mechanism: String,
    pub confidence: f32,
    pub lag_days: i64,
    pub counterfactual_note: String,
}

pub struct CausalChainGenerator;

impl CausalChainGenerator {
    pub fn generate(schema: &WorldSchema, events: &[WorldEvent]) -> Vec<CausalChain> {
        if events.len() < 3 {
            return Vec::new();
        }
        (0..schema.causal_chain_count)
            .map(|chain_index| {
                let start = chain_index % events.len().saturating_sub(2).max(1);
                let chain_events = [
                    events[start % events.len()].id.clone(),
                    events[(start + 1) % events.len()].id.clone(),
                    events[(start + 2) % events.len()].id.clone(),
                ];
                let links = chain_events
                    .windows(2)
                    .enumerate()
                    .map(|(link_index, window)| CausalChainLink {
                        cause_event: window[0].clone(),
                        effect_event: window[1].clone(),
                        mechanism: format!("mechanism-{chain_index}-{link_index}"),
                        confidence: 0.65 + 0.05 * link_index as f32,
                        lag_days: 1 + link_index as i64,
                        counterfactual_note: format!(
                            "Without {}, {} becomes less likely.",
                            window[0], window[1]
                        ),
                    })
                    .collect::<Vec<_>>();
                CausalChain {
                    id: format!("causal-chain-{chain_index:04}"),
                    events: chain_events.to_vec(),
                    links,
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AgentTaskType {
    AnswerQuestions,
    PlanActions,
    DetectContradictions,
    UpdateBeliefs,
    RememberPreferences,
    SimulateOutcomes,
    RecoverTimelines,
    VerifyClaims,
}

impl AgentTaskType {
    pub fn all() -> Vec<Self> {
        vec![
            Self::AnswerQuestions,
            Self::PlanActions,
            Self::DetectContradictions,
            Self::UpdateBeliefs,
            Self::RememberPreferences,
            Self::SimulateOutcomes,
            Self::RecoverTimelines,
            Self::VerifyClaims,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkTask {
    pub id: String,
    pub task_type: AgentTaskType,
    pub prompt: String,
    pub expected_answer: String,
    pub evidence_assertion_ids: Vec<AssertionId>,
    pub hidden_truth_assertion_ids: Vec<AssertionId>,
    pub claim_under_test: Option<ClaimUnderTest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimUnderTest {
    pub assertion_id: AssertionId,
    pub expected_truth: bool,
}

pub struct AgentTaskGenerator;

impl AgentTaskGenerator {
    pub fn generate(
        schema: &WorldSchema,
        truth: &HiddenTrueState,
        contradictions: &[ContradictionPair],
        causal_chains: &[CausalChain],
    ) -> Vec<BenchmarkTask> {
        let types = AgentTaskType::all();
        let mut tasks = Vec::with_capacity(schema.agent_task_count);
        for index in 0..schema.agent_task_count {
            let task_type = types[index % types.len()];
            let truth_assertion = &truth.assertions[index % truth.assertions.len()];
            let evidence_assertion = contradictions
                .get(index % contradictions.len().max(1))
                .map(|pair| pair.observed_false_assertion.clone())
                .unwrap_or_else(|| truth_assertion.id.clone());
            let expected =
                expected_answer_for(task_type, truth_assertion, contradictions, causal_chains);
            tasks.push(BenchmarkTask {
                id: format!("task-{index:04}"),
                task_type,
                prompt: prompt_for(task_type, truth_assertion),
                expected_answer: expected,
                evidence_assertion_ids: vec![evidence_assertion],
                hidden_truth_assertion_ids: vec![truth_assertion.id.clone()],
                claim_under_test: (task_type == AgentTaskType::VerifyClaims).then(|| {
                    ClaimUnderTest {
                        assertion_id: truth_assertion.id.clone(),
                        expected_truth: true,
                    }
                }),
            });
        }
        tasks
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroundTruthOracle {
    answers_by_task: BTreeMap<String, String>,
    true_assertions: BTreeSet<AssertionId>,
    contradictions: Vec<ContradictionPair>,
}

impl GroundTruthOracle {
    pub fn from_world(world: &SyntheticWorld) -> Self {
        Self {
            answers_by_task: world
                .benchmark_tasks
                .iter()
                .map(|task| (task.id.clone(), task.expected_answer.clone()))
                .collect(),
            true_assertions: world
                .hidden_true_state
                .assertions
                .iter()
                .map(|assertion| assertion.id.clone())
                .collect(),
            contradictions: world.noisy_observed_state.contradictions.clone(),
        }
    }

    pub fn answer_task(&self, task_id: &str) -> Option<String> {
        self.answers_by_task.get(task_id).cloned()
    }

    pub fn verify_claim(&self, claim: &ClaimUnderTest) -> bool {
        self.true_assertions.contains(&claim.assertion_id) == claim.expected_truth
    }

    pub fn contradiction_pairs(&self) -> &[ContradictionPair] {
        &self.contradictions
    }
}

fn attach_assertions_to_documents(documents: &mut [SourceDocument], assertions: &[Assertion]) {
    let by_source = assertions
        .iter()
        .flat_map(|assertion| {
            assertion
                .source_ids
                .iter()
                .map(move |source_id| (source_id.clone(), assertion.id.clone()))
        })
        .fold(
            BTreeMap::<SourceId, Vec<AssertionId>>::new(),
            |mut map, (source_id, assertion_id)| {
                map.entry(source_id).or_default().push(assertion_id);
                map
            },
        );
    for document in documents {
        document.supported_assertion_ids = by_source.get(&document.id).cloned().unwrap_or_default();
    }
}

fn rumor_assertions(
    schema: &WorldSchema,
    truth: &HiddenTrueState,
    documents: &[SourceDocument],
) -> Vec<Assertion> {
    let Some(source) = documents
        .iter()
        .find(|document| document.kind == SourceDocumentKind::Rumor)
        .or_else(|| documents.first())
    else {
        return Vec::new();
    };
    let rumor_count = ((truth.assertions.len() as f32 * schema.noise_rate).ceil() as usize).max(1);
    truth
        .assertions
        .iter()
        .take(rumor_count)
        .enumerate()
        .map(|(index, base)| Assertion {
            id: AssertionId::new(format!("rumor-{index:04}")),
            subject: base.subject.clone(),
            predicate: PredicateId::new("RUMORED_RELATIONSHIP"),
            object: GraphValue::Text(format!("unverified claim {}", schema.seed + index as u64)),
            valid_time: base.valid_time.clone(),
            transaction_time: TimeInterval::new(TxTime::new(700 + index as i64), None)
                .expect("valid tx interval"),
            confidence: Confidence::new(0.24).expect("confidence"),
            source_ids: vec![source.id.clone()],
            context: ContextScope::Named("synthetic-world".to_string()),
            status: AssertionStatus::Active,
        })
        .collect()
}

struct AssertionDraft {
    id: String,
    subject: EntityId,
    predicate: &'static str,
    object: GraphValue,
    valid_from: i64,
    valid_to: Option<i64>,
    confidence: f32,
    source_id: SourceId,
}

fn assertion(draft: AssertionDraft) -> Assertion {
    Assertion {
        id: AssertionId::new(draft.id),
        subject: draft.subject,
        predicate: PredicateId::new(draft.predicate),
        object: draft.object,
        valid_time: TimeInterval::new(
            ValidTime::new(draft.valid_from),
            draft.valid_to.map(ValidTime::new),
        )
        .expect("valid time interval"),
        transaction_time: TimeInterval::new(TxTime::new(100), None)
            .expect("valid transaction interval"),
        confidence: Confidence::new(draft.confidence).expect("confidence"),
        source_ids: vec![draft.source_id],
        context: ContextScope::Named("synthetic-world".to_string()),
        status: AssertionStatus::Active,
    }
}

fn contradictory_object(object: &GraphValue, index: usize) -> GraphValue {
    match object {
        GraphValue::Entity(_) => {
            GraphValue::Entity(EntityId::new(format!("decoy-entity-{index:04}")))
        }
        GraphValue::Text(text) => GraphValue::Text(format!("not-{text}")),
        GraphValue::Integer(value) => GraphValue::Integer(value + 1),
        GraphValue::Decimal(value) => GraphValue::Decimal(value + 1.0),
        GraphValue::Boolean(value) => GraphValue::Boolean(!value),
        GraphValue::Time(value) => GraphValue::Time(ValidTime::new(value.as_i64() + 1)),
        GraphValue::Null => GraphValue::Text("contradicted-null".to_string()),
    }
}

fn expected_answer_for(
    task_type: AgentTaskType,
    assertion: &Assertion,
    contradictions: &[ContradictionPair],
    causal_chains: &[CausalChain],
) -> String {
    match task_type {
        AgentTaskType::AnswerQuestions => format!("The answer is supported by {}.", assertion.id),
        AgentTaskType::PlanActions => {
            "Plan action using verified sources before acting on noisy observations.".to_string()
        }
        AgentTaskType::DetectContradictions => {
            format!("Detected {} contradiction(s).", contradictions.len())
        }
        AgentTaskType::UpdateBeliefs => format!(
            "Prefer hidden truth {} over lower-confidence noisy claims.",
            assertion.id
        ),
        AgentTaskType::RememberPreferences => {
            "Remember the user's preference for source-backed answers.".to_string()
        }
        AgentTaskType::SimulateOutcomes => {
            format!(
                "Simulate downstream impact through {} causal chain(s).",
                causal_chains.len()
            )
        }
        AgentTaskType::RecoverTimelines => {
            format!(
                "Recover timeline from valid {}",
                assertion.valid_time.start.as_i64()
            )
        }
        AgentTaskType::VerifyClaims => {
            format!("Claim {} is true in the hidden world.", assertion.id)
        }
    }
}

fn prompt_for(task_type: AgentTaskType, assertion: &Assertion) -> String {
    match task_type {
        AgentTaskType::AnswerQuestions => {
            format!("Answer a question using assertion {}.", assertion.id)
        }
        AgentTaskType::PlanActions => "Plan a safe next action under noisy evidence.".to_string(),
        AgentTaskType::DetectContradictions => {
            "Detect contradictory claims in the observed state.".to_string()
        }
        AgentTaskType::UpdateBeliefs => "Update beliefs after new evidence arrives.".to_string(),
        AgentTaskType::RememberPreferences => {
            "Remember the user's evidence preference.".to_string()
        }
        AgentTaskType::SimulateOutcomes => {
            "Simulate outcome of a disrupted relationship.".to_string()
        }
        AgentTaskType::RecoverTimelines => {
            "Recover the true timeline from source documents.".to_string()
        }
        AgentTaskType::VerifyClaims => {
            format!("Verify claim represented by assertion {}.", assertion.id)
        }
    }
}

fn source_trust(kind: SourceDocumentKind) -> f32 {
    match kind {
        SourceDocumentKind::Contract => 0.92,
        SourceDocumentKind::PolicyChange => 0.88,
        SourceDocumentKind::Document => 0.78,
        SourceDocumentKind::MeetingNote => 0.72,
        SourceDocumentKind::Email => 0.66,
        SourceDocumentKind::News => 0.62,
        SourceDocumentKind::Rumor => 0.22,
    }
}

fn stable_offset(seed: u64, index: usize) -> usize {
    (stable_hash(format!("{seed}:{index}").as_bytes()) % 997) as usize
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
