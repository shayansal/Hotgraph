//! MCP server surface for Reality Graph.

use std::collections::BTreeSet;
use std::fmt;

use serde_json::{json, Map, Value};

use rg_agent_memory::{
    AgentMemoryKind, AgentMemoryService, MemoryPermissions, MemoryQuery, MemoryRetrievalMode,
    WriteMemory,
};
use rg_agent_security::{
    DataProvenance, PermissionScope, PromptInjectionRiskScore, SourceTaintStatus,
    SourceTrustSummary, ToolCallAuditLog, ToolResponseSecurityMetadata,
};
use rg_causal::{CausalGraph, CounterfactualEngine, CounterfactualScenario, Intervention};
use rg_core::{
    AgentId, Assertion, AssertionId, Confidence, Entity, EntityId, EventId, GraphValue, MemoryId,
    MemoryStatus, Source, SourceId, TimeInterval, TxTime, ValidTime,
};
use rg_index::TemporalIndex;
use rg_query::{PathQuery, QueryEngine, QueryResult};
use rg_storage::InMemoryStorage;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonRpcId {
    String(String),
    Number(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpJsonRpcRequest {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum McpResponse {
    Result {
        jsonrpc: String,
        id: JsonRpcId,
        result: Value,
    },
    Error {
        jsonrpc: String,
        id: JsonRpcId,
        error: McpProtocolError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpProtocolError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McpPolicy {
    pub allow_writes: bool,
    pub require_write_confirmation: bool,
}

impl McpPolicy {
    pub fn read_only() -> Self {
        Self {
            allow_writes: false,
            require_write_confirmation: true,
        }
    }

    pub fn allow_writes() -> Self {
        Self {
            allow_writes: true,
            require_write_confirmation: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub description: String,
    pub mime_type: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub mime_type: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub dangerous: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum McpContent {
    Text {
        text: String,
    },
    ResourceLink {
        uri: String,
        name: String,
        description: String,
        mime_type: String,
    },
}

impl McpContent {
    fn to_json(&self) -> Value {
        match self {
            Self::Text { text } => json!({
                "type": "text",
                "text": text,
            }),
            Self::ResourceLink {
                uri,
                name,
                description,
                mime_type,
            } => json!({
                "type": "resource_link",
                "uri": uri,
                "name": name,
                "description": description,
                "mimeType": mime_type,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    pub structured_content: Value,
    pub is_error: bool,
    pub security: ToolResponseSecurityMetadata,
}

impl McpToolResult {
    pub fn content_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|content| match content {
                McpContent::Text { text } => Some(text.as_str()),
                McpContent::ResourceLink { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn to_protocol_value(&self) -> Value {
        let security = self.security.to_json();
        let structured_content = with_security(self.structured_content.clone(), security.clone());
        let mut value = json!({
            "content": self.content.iter().map(McpContent::to_json).collect::<Vec<_>>(),
            "structuredContent": structured_content,
            "security": security,
        });
        if self.is_error {
            value["isError"] = Value::Bool(true);
        }
        value
    }

    fn ok(summary: impl Into<String>, structured_content: Value, links: Vec<McpContent>) -> Self {
        let mut content = vec![McpContent::Text {
            text: summary.into(),
        }];
        content.extend(links);
        Self {
            content,
            structured_content,
            is_error: false,
            security: default_security_metadata("unknown_tool"),
        }
    }

    fn execution_error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            content: vec![McpContent::Text {
                text: message.clone(),
            }],
            structured_content: json!({ "error": message }),
            is_error: true,
            security: default_security_metadata("unknown_tool"),
        }
    }

    fn with_security(mut self, security: ToolResponseSecurityMetadata) -> Self {
        self.structured_content = with_security(self.structured_content, security.to_json());
        self.security = security;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpError {
    UnknownResource(String),
    UnknownTool(String),
    InvalidArguments(String),
    ReadOnlyPolicy,
    MemoryWrite(String),
}

impl fmt::Display for McpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownResource(uri) => write!(formatter, "unknown resource: {uri}"),
            Self::UnknownTool(name) => write!(formatter, "unknown tool: {name}"),
            Self::InvalidArguments(message) => write!(formatter, "invalid arguments: {message}"),
            Self::ReadOnlyPolicy => formatter.write_str("write policy forbids this tool"),
            Self::MemoryWrite(message) => write!(formatter, "memory write failed: {message}"),
        }
    }
}

impl std::error::Error for McpError {}

#[derive(Clone, Debug, PartialEq)]
pub struct McpServer {
    storage: InMemoryStorage,
    memories: AgentMemoryService,
    causal_graph: CausalGraph,
    policy: McpPolicy,
    audit_log: ToolCallAuditLog,
}

impl McpServer {
    pub fn new(
        storage: InMemoryStorage,
        memories: AgentMemoryService,
        causal_graph: CausalGraph,
        policy: McpPolicy,
    ) -> Self {
        Self {
            storage,
            memories,
            causal_graph,
            policy,
            audit_log: ToolCallAuditLog::new("mcp-audit"),
        }
    }

    pub fn resource_templates(&self) -> Vec<McpResourceTemplate> {
        vec![
            template(
                "graph://entities/{id}",
                "Entity",
                "Entity state and evidence",
            ),
            template(
                "graph://assertions/{id}",
                "Assertion",
                "Assertion provenance",
            ),
            template("graph://sources/{id}", "Source", "Source metadata"),
            template(
                "graph://memories/{agent_id}",
                "Agent Memories",
                "Agent memories",
            ),
            template(
                "graph://timelines/{entity_id}",
                "Timeline",
                "Entity assertion timeline",
            ),
        ]
    }

    pub fn tools(&self) -> Vec<McpTool> {
        let mut tools = vec![
            tool(
                "search_context",
                "Search Context",
                "Search source-backed graph and agent memory context.",
                object_schema(
                    vec![
                        ("query", string_schema("Search phrase")),
                        ("agent_id", string_schema("Optional agent ID")),
                        ("limit", integer_schema("Maximum results")),
                    ],
                    vec!["query"],
                ),
                false,
            ),
            tool(
                "get_evidence_pack",
                "Get Evidence Pack",
                "Build compact source-backed evidence for an entity.",
                object_schema(
                    vec![
                        ("entity_id", string_schema("Entity ID")),
                        ("question", string_schema("Question or task")),
                        ("valid_at", integer_schema("Valid-time timestamp")),
                    ],
                    vec!["entity_id"],
                ),
                false,
            ),
            tool(
                "get_entity_timeline",
                "Get Entity Timeline",
                "Return temporal assertions for an entity.",
                object_schema(
                    vec![
                        ("entity_id", string_schema("Entity ID")),
                        ("valid_at", integer_schema("Optional valid-time filter")),
                    ],
                    vec!["entity_id"],
                ),
                false,
            ),
            tool(
                "find_paths",
                "Find Paths",
                "Find graph paths with provenance.",
                object_schema(
                    vec![
                        ("start_entity_id", string_schema("Start entity ID")),
                        ("end_entity_id", string_schema("Optional end entity ID")),
                        ("max_depth", integer_schema("Maximum depth")),
                        ("valid_at", integer_schema("Optional valid time")),
                    ],
                    vec!["start_entity_id"],
                ),
                false,
            ),
            tool(
                "detect_conflicts",
                "Detect Conflicts",
                "Detect contradictory assertions.",
                object_schema(
                    vec![("entity_id", string_schema("Optional entity ID"))],
                    Vec::new(),
                ),
                false,
            ),
            tool(
                "run_counterfactual",
                "Run Counterfactual",
                "Run a non-factual counterfactual simulation.",
                object_schema(
                    vec![
                        (
                            "intervention_type",
                            enum_schema(
                                "Intervention kind",
                                vec!["remove_assertion", "remove_event"],
                            ),
                        ),
                        ("assertion_id", string_schema("Assertion ID")),
                        ("event_id", string_schema("Event ID")),
                        ("max_depth", integer_schema("Maximum depth")),
                        ("valid_at", integer_schema("Valid time")),
                    ],
                    vec!["intervention_type"],
                ),
                false,
            ),
            tool(
                "write_memory",
                "Write Memory",
                "Write source-backed agent memory. Requires explicit policy and confirmation.",
                write_memory_schema(),
                true,
            ),
        ];
        tools.extend(alias_tools());
        tools
    }

    pub fn read_resource(&self, uri: &str) -> Result<McpResource, McpError> {
        if let Some(id) = uri.strip_prefix("graph://entities/") {
            return self.entity_resource(EntityId::new(id));
        }
        if let Some(id) = uri.strip_prefix("graph://assertions/") {
            return self.assertion_resource(AssertionId::new(id));
        }
        if let Some(id) = uri.strip_prefix("graph://sources/") {
            return self.source_resource(SourceId::new(id));
        }
        if let Some(id) = uri.strip_prefix("graph://memories/") {
            return Ok(self.memories_resource(AgentId::new(id)));
        }
        if let Some(id) = uri.strip_prefix("graph://timelines/") {
            return Ok(self.timeline_resource(EntityId::new(id), None));
        }
        Err(McpError::UnknownResource(uri.to_owned()))
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<McpToolResult, McpError> {
        let canonical = canonical_tool_name(name);
        let tool = self
            .tools()
            .into_iter()
            .find(|tool| tool.name == canonical)
            .ok_or_else(|| McpError::UnknownTool(name.to_owned()))?;
        if let Err(error) = validate_arguments(&tool.input_schema, &arguments) {
            let result = McpToolResult::execution_error(error);
            return Ok(self.secure_denied_result(canonical, result, "schema validation failed"));
        }

        let risk = PromptInjectionRiskScore::assess_json(&arguments);
        if risk.quarantine_recommended() {
            let result = McpToolResult::execution_error(
                "prompt injection quarantine: tool call was not executed",
            );
            return Ok(self.secure_quarantine_result(canonical, result, &risk));
        }

        match canonical {
            "search_context" => {
                let result = self.search_context(arguments);
                Ok(self.secure_result(canonical, result))
            }
            "get_evidence_pack" => {
                let result = self.get_evidence_pack(arguments);
                Ok(self.secure_result(canonical, result))
            }
            "get_entity_timeline" => {
                let result = self.get_entity_timeline(arguments);
                Ok(self.secure_result(canonical, result))
            }
            "find_paths" => {
                let result = self.find_paths(arguments);
                Ok(self.secure_result(canonical, result))
            }
            "detect_conflicts" => {
                let result = self.detect_conflicts(arguments);
                Ok(self.secure_result(canonical, result))
            }
            "run_counterfactual" => {
                let result = self.run_counterfactual(arguments);
                Ok(self.secure_result(canonical, result))
            }
            "write_memory" => {
                let result = self.write_memory(arguments);
                Ok(self.secure_result(canonical, result))
            }
            other => Err(McpError::UnknownTool(other.to_owned())),
        }
    }

    pub fn handle_request(&mut self, request: McpJsonRpcRequest) -> McpResponse {
        if request.jsonrpc != "2.0" {
            return protocol_error(request.id, -32600, "jsonrpc must be 2.0");
        }

        match request.method.as_str() {
            "tools/list" => protocol_result(
                request.id,
                json!({
                    "tools": self.tools().into_iter().map(tool_to_json).collect::<Vec<_>>()
                }),
            ),
            "resources/templates/list" => protocol_result(
                request.id,
                json!({
                    "resourceTemplates": self
                        .resource_templates()
                        .into_iter()
                        .map(template_to_json)
                        .collect::<Vec<_>>()
                }),
            ),
            "resources/read" => {
                let Some(uri) = request.params.get("uri").and_then(Value::as_str) else {
                    return protocol_error(request.id, -32602, "resources/read requires uri");
                };
                match self.read_resource(uri) {
                    Ok(resource) => protocol_result(request.id, resource_to_json(resource)),
                    Err(error) => protocol_error(request.id, -32602, error.to_string()),
                }
            }
            "tools/call" => {
                let Some(name) = request.params.get("name").and_then(Value::as_str) else {
                    return protocol_error(request.id, -32602, "tools/call requires name");
                };
                let arguments = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match self.call_tool(name, arguments) {
                    Ok(result) => protocol_result(request.id, result.to_protocol_value()),
                    Err(error) => protocol_error(request.id, -32602, error.to_string()),
                }
            }
            _ => protocol_error(request.id, -32601, "unknown MCP method"),
        }
    }

    fn entity_resource(&self, entity_id: EntityId) -> Result<McpResource, McpError> {
        let Some(entity) = self.storage.graph_state().entities.get(&entity_id) else {
            return Err(McpError::UnknownResource(format!(
                "graph://entities/{entity_id}"
            )));
        };
        let assertions = self.assertions_for_entity(&entity_id, None);
        let mut text = compact_entity(entity);
        for assertion in assertions {
            text.push('\n');
            text.push_str(&compact_assertion(assertion));
        }
        Ok(resource(
            format!("graph://entities/{entity_id}"),
            format!("Entity {entity_id}"),
            text,
        ))
    }

    fn assertion_resource(&self, assertion_id: AssertionId) -> Result<McpResource, McpError> {
        let Some(assertion) = self.storage.graph_state().assertions.get(&assertion_id) else {
            return Err(McpError::UnknownResource(format!(
                "graph://assertions/{assertion_id}"
            )));
        };
        Ok(resource(
            format!("graph://assertions/{assertion_id}"),
            format!("Assertion {assertion_id}"),
            compact_assertion(assertion),
        ))
    }

    fn source_resource(&self, source_id: SourceId) -> Result<McpResource, McpError> {
        let Some(source) = self.storage.graph_state().sources.get(&source_id) else {
            return Err(McpError::UnknownResource(format!(
                "graph://sources/{source_id}"
            )));
        };
        Ok(resource(
            format!("graph://sources/{source_id}"),
            format!("Source {source_id}"),
            compact_source(source),
        ))
    }

    fn memories_resource(&self, agent_id: AgentId) -> McpResource {
        let retrieval = self.memories.retrieve_memory(MemoryQuery {
            agent_id: agent_id.clone(),
            query: String::new(),
            valid_at: None,
            related_entities: Vec::new(),
            include_history: true,
            mode: MemoryRetrievalMode::GraphTemporal,
            limit: None,
        });
        let mut lines = vec![format!("agent_id={agent_id}")];
        for memory in retrieval.memories {
            lines.push(format!(
                "memory_id={} type={:?} status={:?} confidence={:.2} source_ids={} content={}",
                memory.record.id,
                memory.record.memory_type,
                memory.record.lifecycle,
                memory.record.confidence.as_f32(),
                join_ids(&memory.record.source_ids),
                memory.record.content
            ));
        }
        resource(
            format!("graph://memories/{agent_id}"),
            format!("Memories {agent_id}"),
            lines.join("\n"),
        )
    }

    fn timeline_resource(&self, entity_id: EntityId, valid_at: Option<ValidTime>) -> McpResource {
        let mut assertions = self.assertions_for_entity(&entity_id, valid_at);
        assertions.sort_by(|left, right| {
            left.valid_time
                .start
                .cmp(&right.valid_time.start)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut lines = vec![format!("entity_id={entity_id}")];
        for assertion in assertions {
            lines.push(compact_assertion(assertion));
        }
        resource(
            format!("graph://timelines/{entity_id}"),
            format!("Timeline {entity_id}"),
            lines.join("\n"),
        )
    }

    fn search_context(&self, arguments: Value) -> McpToolResult {
        let query = required_string(&arguments, "query").unwrap_or_default();
        let limit = optional_usize(&arguments, "limit").unwrap_or(5);
        let agent_id = optional_string(&arguments, "agent_id")
            .map(AgentId::new)
            .unwrap_or_else(|| AgentId::new("agent"));
        let memory_results = self.memories.retrieve_memory(MemoryQuery {
            agent_id: agent_id.clone(),
            query: query.clone(),
            valid_at: None,
            related_entities: Vec::new(),
            include_history: false,
            mode: MemoryRetrievalMode::GraphTemporal,
            limit: Some(limit),
        });

        let memories = memory_results
            .memories
            .iter()
            .map(|memory| {
                json!({
                    "memory_id": memory.record.id.to_string(),
                    "content": memory.record.content,
                    "score": memory.score,
                    "source_ids": memory.record.source_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "current_truth": memory.current_truth,
                    "explanation": memory.explanation,
                })
            })
            .collect::<Vec<_>>();
        McpToolResult::ok(
            format!(
                "Retrieved {} compact context items with source IDs.",
                memories.len()
            ),
            json!({
                "query": query,
                "agent_id": agent_id.to_string(),
                "memories": memories,
                "quality_score": memory_results.quality_score,
            }),
            vec![resource_link(
                format!("graph://memories/{agent_id}"),
                "Agent memories",
                "Memory resource for retrieved context",
            )],
        )
    }

    fn get_evidence_pack(&self, arguments: Value) -> McpToolResult {
        let entity_id = EntityId::new(required_string(&arguments, "entity_id").unwrap_or_default());
        let valid_at = optional_i64(&arguments, "valid_at").map(ValidTime::new);
        let assertions = self.assertions_for_entity(&entity_id, valid_at);
        let source_ids = assertions
            .iter()
            .flat_map(|assertion| assertion.source_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let sources = source_ids
            .iter()
            .filter_map(|source_id| self.storage.graph_state().sources.get(source_id))
            .map(source_json)
            .collect::<Vec<_>>();
        let assertion_values = assertions.iter().map(assertion_json).collect::<Vec<_>>();
        McpToolResult::ok(
            format!(
                "Evidence pack for {entity_id}: {} assertions, {} sources.",
                assertion_values.len(),
                sources.len()
            ),
            json!({
                "entity_id": entity_id.to_string(),
                "question": optional_string(&arguments, "question"),
                "assertions": assertion_values,
                "sources": sources,
                "contradiction_warnings": self.contradictions_for_entity(Some(&entity_id)).len(),
            }),
            vec![resource_link(
                format!("graph://entities/{entity_id}"),
                "Entity state",
                "Entity resource with linked assertions and source IDs",
            )],
        )
    }

    fn get_entity_timeline(&self, arguments: Value) -> McpToolResult {
        let entity_id = EntityId::new(required_string(&arguments, "entity_id").unwrap_or_default());
        let valid_at = optional_i64(&arguments, "valid_at").map(ValidTime::new);
        let resource = self.timeline_resource(entity_id.clone(), valid_at);
        let assertions = self
            .assertions_for_entity(&entity_id, valid_at)
            .iter()
            .map(assertion_json)
            .collect::<Vec<_>>();
        McpToolResult::ok(
            format!("Timeline for {entity_id}: {} assertions.", assertions.len()),
            json!({
                "entity_id": entity_id.to_string(),
                "assertions": assertions,
            }),
            vec![resource_link(
                resource.uri,
                resource.name,
                "Timeline resource",
            )],
        )
    }

    fn find_paths(&self, arguments: Value) -> McpToolResult {
        let start =
            EntityId::new(required_string(&arguments, "start_entity_id").unwrap_or_default());
        let end = optional_string(&arguments, "end_entity_id").map(EntityId::new);
        let max_depth = optional_usize(&arguments, "max_depth").unwrap_or(2);
        let valid_at = optional_i64(&arguments, "valid_at");
        let engine = QueryEngine::from_storage(self.storage.clone());
        let results = engine.execute_path(PathQuery {
            start: start.clone(),
            end: end.clone(),
            predicates: Vec::new(),
            valid_at,
            max_depth,
            min_confidence: None,
        });
        let paths = results
            .iter()
            .map(|path| {
                json!({
                    "start": path.start.to_string(),
                    "end": path.end.to_string(),
                    "hops": path.hops.iter().map(query_result_json).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        McpToolResult::ok(
            format!("Found {} source-backed graph paths.", paths.len()),
            json!({
                "start_entity_id": start.to_string(),
                "end_entity_id": end.map(|id| id.to_string()),
                "path_count": paths.len(),
                "paths": paths,
            }),
            vec![resource_link(
                format!("graph://entities/{start}"),
                "Start entity",
                "Start entity resource",
            )],
        )
    }

    fn detect_conflicts(&self, arguments: Value) -> McpToolResult {
        let entity_id = optional_string(&arguments, "entity_id").map(EntityId::new);
        let conflicts = self.contradictions_for_entity(entity_id.as_ref());
        let values = conflicts
            .iter()
            .map(|contradiction| {
                json!({
                    "id": contradiction.id.to_string(),
                    "assertion_ids": [
                        contradiction.assertion_a.to_string(),
                        contradiction.assertion_b.to_string()
                    ],
                    "type": contradiction.contradiction_type.to_string(),
                    "severity": contradiction.severity.to_string(),
                    "explanation": contradiction.explanation,
                })
            })
            .collect::<Vec<_>>();
        McpToolResult::ok(
            format!("Detected {} contradiction clusters.", values.len()),
            json!({
                "entity_id": entity_id.map(|id| id.to_string()),
                "conflict_count": values.len(),
                "conflicts": values,
            }),
            Vec::new(),
        )
    }

    fn run_counterfactual(&self, arguments: Value) -> McpToolResult {
        let intervention_type =
            required_string(&arguments, "intervention_type").unwrap_or_default();
        let max_depth = optional_usize(&arguments, "max_depth").unwrap_or(3);
        let valid_at = ValidTime::new(optional_i64(&arguments, "valid_at").unwrap_or(0));
        let intervention = match intervention_type.as_str() {
            "remove_assertion" => {
                let assertion_id = AssertionId::new(
                    required_string(&arguments, "assertion_id").unwrap_or_default(),
                );
                Intervention::RemoveAssertion(assertion_id)
            }
            "remove_event" => {
                let event_id =
                    EventId::new(required_string(&arguments, "event_id").unwrap_or_default());
                Intervention::RemoveEvent(event_id)
            }
            _ => {
                return McpToolResult::execution_error(
                    "intervention_type must be remove_assertion or remove_event",
                );
            }
        };
        let trace = CounterfactualEngine::new(&self.causal_graph, self.storage.graph_state())
            .simulate(CounterfactualScenario {
                intervention,
                valid_at,
                max_depth,
                assumptions: vec!["MCP counterfactual output is simulation, not fact.".to_owned()],
            });
        McpToolResult::ok(
            "Counterfactual simulation completed; output is not asserted as fact.",
            json!({
                "simulation_not_fact": trace.simulation_not_fact,
                "affected_entities": ids_json(&trace.affected_entities),
                "affected_assertions": ids_json(&trace.affected_assertions),
                "affected_events": ids_json(&trace.affected_events),
                "uncertainty": trace.uncertainty,
                "explanation_trace": trace.explanation_trace,
            }),
            Vec::new(),
        )
    }

    fn write_memory(&mut self, arguments: Value) -> McpToolResult {
        if !self.policy.allow_writes {
            return McpToolResult::execution_error(
                "write policy forbids write_memory; enable writes explicitly for this MCP server",
            );
        }
        if self.policy.require_write_confirmation
            && arguments.get("confirm_write").and_then(Value::as_bool) != Some(true)
        {
            return McpToolResult::execution_error(
                "write_memory requires confirm_write=true because it changes agent memory",
            );
        }

        let agent_id = AgentId::new(required_string(&arguments, "agent_id").unwrap_or_default());
        let memory_id = MemoryId::new(required_string(&arguments, "memory_id").unwrap_or_default());
        let memory_type = parse_memory_kind(
            &required_string(&arguments, "memory_type").unwrap_or_else(|| "Observation".to_owned()),
        );
        let valid_from = ValidTime::new(optional_i64(&arguments, "valid_from").unwrap_or(0));
        let confidence = optional_f32(&arguments, "confidence")
            .and_then(|value| Confidence::new(value).ok())
            .unwrap_or_else(|| Confidence::new(0.5).expect("default confidence is valid"));
        let source_ids = string_array(&arguments, "source_ids")
            .into_iter()
            .map(SourceId::new)
            .collect::<Vec<_>>();
        let write = WriteMemory {
            id: memory_id,
            agent_id: agent_id.clone(),
            memory_type,
            content: required_string(&arguments, "content").unwrap_or_default(),
            valid_time: TimeInterval::new(valid_from, None).expect("open interval is valid"),
            confidence,
            source_ids,
            related_entities: string_array(&arguments, "related_entity_ids")
                .into_iter()
                .map(EntityId::new)
                .collect(),
            supersedes: Vec::new(),
            contradicts: Vec::new(),
            lifecycle: MemoryStatus::Active,
            permissions: MemoryPermissions::private(agent_id.clone()),
        };
        match self.memories.write_memory(write) {
            Ok(record) => McpToolResult::ok(
                format!(
                    "Memory {} written with explicit confirmation and source IDs.",
                    record.id
                ),
                json!({
                    "memory_id": record.id.to_string(),
                    "agent_id": record.agent_id.to_string(),
                    "source_ids": record.source_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "status": format!("{:?}", record.lifecycle),
                }),
                vec![resource_link(
                    format!("graph://memories/{agent_id}"),
                    "Agent memories",
                    "Updated memory resource",
                )],
            ),
            Err(error) => McpToolResult::execution_error(format!("memory write failed: {error}")),
        }
    }

    fn assertions_for_entity(
        &self,
        entity_id: &EntityId,
        valid_at: Option<ValidTime>,
    ) -> Vec<&Assertion> {
        let mut assertions = self
            .storage
            .graph_state()
            .assertions
            .values()
            .filter(|assertion| {
                &assertion.subject == entity_id
                    || matches!(&assertion.object, GraphValue::Entity(object) if object == entity_id)
            })
            .filter(|assertion| valid_at.map_or(true, |instant| assertion.valid_time.contains(instant)))
            .collect::<Vec<_>>();
        assertions.sort_by(|left, right| left.id.cmp(&right.id));
        assertions
    }

    fn contradictions_for_entity(
        &self,
        entity_id: Option<&EntityId>,
    ) -> Vec<rg_index::Contradiction> {
        let mut index = TemporalIndex::new();
        for assertion in self.storage.graph_state().assertions.values() {
            index.insert_assertion(assertion.clone());
        }
        let mut contradictions = index.contradictions();
        if let Some(entity_id) = entity_id {
            contradictions.retain(|contradiction| {
                [&contradiction.assertion_a, &contradiction.assertion_b]
                    .iter()
                    .filter_map(|assertion_id| {
                        self.storage.graph_state().assertions.get(assertion_id)
                    })
                    .any(|assertion| &assertion.subject == entity_id)
            });
        }
        contradictions
    }

    fn secure_result(&mut self, tool_name: &str, result: McpToolResult) -> McpToolResult {
        let provenance = DataProvenance::from_json(tool_name, &result.structured_content);
        let trust = self.source_trust(&provenance.source_ids);
        let audit = if result.is_error {
            self.audit_log.record_denied(
                AgentId::new("mcp-client"),
                tool_name,
                provenance.source_ids.clone(),
                "tool execution error",
                TxTime::new(0),
            )
        } else {
            self.audit_log.record_allowed(
                AgentId::new("mcp-client"),
                tool_name,
                provenance.source_ids.clone(),
                TxTime::new(0),
            )
        };
        let security = if result.is_error {
            ToolResponseSecurityMetadata::denied(tool_name, audit.id)
        } else {
            ToolResponseSecurityMetadata::tool(tool_name, provenance, trust, audit.id)
        };
        result.with_security(security)
    }

    fn secure_denied_result(
        &mut self,
        tool_name: &str,
        result: McpToolResult,
        reason: &str,
    ) -> McpToolResult {
        let audit = self.audit_log.record_denied(
            AgentId::new("mcp-client"),
            tool_name,
            Vec::new(),
            reason,
            TxTime::new(0),
        );
        result.with_security(ToolResponseSecurityMetadata::denied(tool_name, audit.id))
    }

    fn secure_quarantine_result(
        &mut self,
        tool_name: &str,
        result: McpToolResult,
        risk: &PromptInjectionRiskScore,
    ) -> McpToolResult {
        let audit = self.audit_log.record_denied(
            AgentId::new("mcp-client"),
            tool_name,
            Vec::new(),
            "prompt injection quarantine",
            TxTime::new(0),
        );
        result.with_security(ToolResponseSecurityMetadata::quarantine(
            tool_name, risk, audit.id,
        ))
    }

    fn source_trust(&self, source_ids: &[SourceId]) -> SourceTrustSummary {
        SourceTrustSummary::from_scores(
            source_ids
                .iter()
                .map(|source_id| {
                    self.storage
                        .graph_state()
                        .sources
                        .get(source_id)
                        .and_then(|source| source.trust_score)
                })
                .collect(),
        )
    }
}

fn template(
    uri_template: impl Into<String>,
    name: impl Into<String>,
    description: impl Into<String>,
) -> McpResourceTemplate {
    McpResourceTemplate {
        uri_template: uri_template.into(),
        name: name.into(),
        description: description.into(),
        mime_type: "text/plain".to_owned(),
    }
}

fn with_security(mut value: Value, security: Value) -> Value {
    match &mut value {
        Value::Object(map) => {
            map.insert("security".to_owned(), security);
            value
        }
        _ => json!({
            "value": value,
            "security": security,
        }),
    }
}

fn default_security_metadata(tool_name: &str) -> ToolResponseSecurityMetadata {
    ToolResponseSecurityMetadata {
        permission_scope: PermissionScope::Tool {
            tool_name: tool_name.to_owned(),
        },
        data_provenance: DataProvenance::empty(),
        taint_status: SourceTaintStatus::trusted(),
        source_trust: SourceTrustSummary::unknown(),
        audit_event_id: "mcp-audit-unrecorded".to_owned(),
    }
}

fn resource(uri: impl Into<String>, name: impl Into<String>, text: String) -> McpResource {
    McpResource {
        uri: uri.into(),
        name: name.into(),
        mime_type: "text/plain".to_owned(),
        text,
    }
}

fn resource_link(
    uri: impl Into<String>,
    name: impl Into<String>,
    description: impl Into<String>,
) -> McpContent {
    McpContent::ResourceLink {
        uri: uri.into(),
        name: name.into(),
        description: description.into(),
        mime_type: "text/plain".to_owned(),
    }
}

fn tool(
    name: impl Into<String>,
    title: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
    dangerous: bool,
) -> McpTool {
    McpTool {
        name: name.into(),
        title: title.into(),
        description: description.into(),
        input_schema,
        output_schema: Some(json!({
            "type": "object",
            "additionalProperties": true,
        })),
        dangerous,
    }
}

fn alias_tools() -> Vec<McpTool> {
    vec![
        alias_tool("search_memory", "search_context"),
        alias_tool("retrieve_context_for_task", "search_context"),
        alias_tool("query_graph", "get_evidence_pack"),
        alias_tool("get_entity_state", "get_evidence_pack"),
        alias_tool("get_timeline", "get_entity_timeline"),
        alias_tool("find_contradictions", "detect_conflicts"),
        alias_tool("simulate_counterfactual", "run_counterfactual"),
        alias_tool("write_agent_memory", "write_memory"),
    ]
}

fn alias_tool(name: &str, canonical: &str) -> McpTool {
    let canonical_schema = match canonical {
        "write_memory" => write_memory_schema(),
        "run_counterfactual" => object_schema(
            vec![
                (
                    "intervention_type",
                    enum_schema(
                        "Intervention kind",
                        vec!["remove_assertion", "remove_event"],
                    ),
                ),
                ("assertion_id", string_schema("Assertion ID")),
                ("event_id", string_schema("Event ID")),
                ("max_depth", integer_schema("Maximum depth")),
                ("valid_at", integer_schema("Valid time")),
            ],
            vec!["intervention_type"],
        ),
        _ => object_schema(Vec::new(), Vec::new()),
    };
    tool(
        name,
        name,
        format!("Alias for {canonical}."),
        canonical_schema,
        canonical == "write_memory",
    )
}

fn canonical_tool_name(name: &str) -> &str {
    match name {
        "search_memory" | "retrieve_context_for_task" => "search_context",
        "query_graph" | "get_entity_state" => "get_evidence_pack",
        "get_timeline" => "get_entity_timeline",
        "find_contradictions" => "detect_conflicts",
        "simulate_counterfactual" => "run_counterfactual",
        "write_agent_memory" => "write_memory",
        other => other,
    }
}

fn object_schema(properties: Vec<(&str, Value)>, required: Vec<&str>) -> Value {
    let mut property_map = Map::new();
    for (name, schema) in properties {
        property_map.insert(name.to_owned(), schema);
    }
    json!({
        "type": "object",
        "properties": property_map,
        "required": required,
        "additionalProperties": false,
    })
}

fn string_schema(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn integer_schema(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn number_schema(description: &str) -> Value {
    json!({ "type": "number", "description": description })
}

fn boolean_const_schema(description: &str, value: bool) -> Value {
    json!({ "type": "boolean", "const": value, "description": description })
}

fn enum_schema(description: &str, values: Vec<&str>) -> Value {
    json!({ "type": "string", "enum": values, "description": description })
}

fn write_memory_schema() -> Value {
    object_schema(
        vec![
            ("agent_id", string_schema("Agent ID")),
            ("memory_id", string_schema("Memory ID")),
            ("memory_type", string_schema("Memory type")),
            ("content", string_schema("Memory content")),
            ("valid_from", integer_schema("Valid start time")),
            ("confidence", number_schema("Confidence 0..1")),
            (
                "source_ids",
                json!({
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Source IDs proving the memory"
                }),
            ),
            (
                "related_entity_ids",
                json!({
                    "type": "array",
                    "items": { "type": "string" }
                }),
            ),
            (
                "confirm_write",
                boolean_const_schema("Must be true for dangerous writes", true),
            ),
        ],
        vec![
            "agent_id",
            "memory_id",
            "memory_type",
            "content",
            "valid_from",
            "confidence",
            "source_ids",
        ],
    )
}

fn validate_arguments(schema: &Value, arguments: &Value) -> Result<(), String> {
    let Some(args) = arguments.as_object() else {
        return Err("arguments must be an object".to_owned());
    };
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in args.keys() {
        if !properties.contains_key(key) {
            return Err(format!("unexpected property {key}"));
        }
    }
    for required in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !args.contains_key(required) {
            return Err(format!("missing required property {required}"));
        }
    }
    for (key, property_schema) in &properties {
        if let Some(value) = args.get(key) {
            validate_value_type(key, property_schema, value)?;
        }
    }
    Ok(())
}

fn validate_value_type(key: &str, schema: &Value, value: &Value) -> Result<(), String> {
    if let Some(const_value) = schema.get("const") {
        if value != const_value {
            return Err(format!("{key} must be {const_value}"));
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") if !value.is_string() => Err(format!("{key} must be a string")),
        Some("integer") if value.as_i64().is_none() => Err(format!("{key} must be an integer")),
        Some("number") if !value.is_number() => Err(format!("{key} must be a number")),
        Some("boolean") if !value.is_boolean() => Err(format!("{key} must be a boolean")),
        Some("array") if !value.is_array() => Err(format!("{key} must be an array")),
        _ => Ok(()),
    }
}

fn compact_entity(entity: &Entity) -> String {
    format!(
        "entity_id={} type={:?} name={} created_tx={}",
        entity.id,
        entity.entity_type,
        entity
            .canonical_name
            .clone()
            .unwrap_or_else(|| "<unnamed>".to_owned()),
        entity.created_tx.as_i64()
    )
}

fn compact_assertion(assertion: &Assertion) -> String {
    format!(
        "assertion_id={} subject={} predicate={} object={} valid_from={} valid_to={} confidence={:.2} source_ids={}",
        assertion.id,
        assertion.subject,
        assertion.predicate,
        graph_value_text(&assertion.object),
        assertion.valid_time.start.as_i64(),
        assertion
            .valid_time
            .end
            .map(|time| time.as_i64().to_string())
            .unwrap_or_else(|| "open".to_owned()),
        assertion.confidence.as_f32(),
        join_ids(&assertion.source_ids)
    )
}

fn compact_source(source: &Source) -> String {
    format!(
        "source_id={} type={:?} uri={} content_hash={} observed_at={} trust_score={}",
        source.id,
        source.source_type,
        source.uri.clone().unwrap_or_else(|| "<none>".to_owned()),
        source.content_hash,
        source.observed_at.as_i64(),
        source
            .trust_score
            .map(|score| format!("{score:.2}"))
            .unwrap_or_else(|| "unknown".to_owned())
    )
}

fn assertion_json(assertion: &&Assertion) -> Value {
    json!({
        "assertion_id": assertion.id.to_string(),
        "subject": assertion.subject.to_string(),
        "predicate": assertion.predicate.to_string(),
        "object": graph_value_text(&assertion.object),
        "valid_from": assertion.valid_time.start.as_i64(),
        "valid_to": assertion.valid_time.end.map(ValidTime::as_i64),
        "confidence": assertion.confidence.as_f32(),
        "source_ids": assertion.source_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
    })
}

fn query_result_json(result: &QueryResult) -> Value {
    json!({
        "assertion_id": result.assertion_id.to_string(),
        "subject": result.subject.to_string(),
        "predicate": result.predicate.to_string(),
        "object": graph_value_text(&result.object),
        "valid_from": result.valid_from.as_i64(),
        "valid_to": result.valid_to.map(ValidTime::as_i64),
        "confidence": result.confidence.as_f32(),
        "source_ids": result.source_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
    })
}

fn source_json(source: &Source) -> Value {
    json!({
        "source_id": source.id.to_string(),
        "uri": source.uri,
        "content_hash": source.content_hash.to_string(),
        "trust_score": source.trust_score,
    })
}

fn graph_value_text(value: &GraphValue) -> String {
    match value {
        GraphValue::Entity(id) => id.to_string(),
        GraphValue::Text(text) => text.clone(),
        GraphValue::Integer(value) => value.to_string(),
        GraphValue::Decimal(value) => value.to_string(),
        GraphValue::Boolean(value) => value.to_string(),
        GraphValue::Time(value) => value.as_i64().to_string(),
        GraphValue::Null => "null".to_owned(),
    }
}

fn join_ids<T: ToString>(ids: &[T]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn ids_json<T: ToString>(ids: &[T]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn required_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    required_string(arguments, key)
}

fn optional_i64(arguments: &Value, key: &str) -> Option<i64> {
    arguments.get(key).and_then(Value::as_i64)
}

fn optional_usize(arguments: &Value, key: &str) -> Option<usize> {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn optional_f32(arguments: &Value, key: &str) -> Option<f32> {
    arguments
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
}

fn string_array(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_memory_kind(value: &str) -> AgentMemoryKind {
    match value {
        "Episodic" => AgentMemoryKind::Episodic,
        "Semantic" => AgentMemoryKind::Semantic,
        "Procedural" => AgentMemoryKind::Procedural,
        "Preference" => AgentMemoryKind::Preference,
        "Goal" => AgentMemoryKind::Goal,
        "Plan" => AgentMemoryKind::Plan,
        "Reflection" => AgentMemoryKind::Reflection,
        "Correction" => AgentMemoryKind::Correction,
        "Relationship" => AgentMemoryKind::Relationship,
        "WorldState" => AgentMemoryKind::WorldState,
        _ => AgentMemoryKind::Episodic,
    }
}

fn protocol_result(id: JsonRpcId, result: Value) -> McpResponse {
    McpResponse::Result {
        jsonrpc: "2.0".to_owned(),
        id,
        result,
    }
}

fn protocol_error(id: JsonRpcId, code: i64, message: impl Into<String>) -> McpResponse {
    McpResponse::Error {
        jsonrpc: "2.0".to_owned(),
        id,
        error: McpProtocolError {
            code,
            message: message.into(),
        },
    }
}

fn template_to_json(template: McpResourceTemplate) -> Value {
    json!({
        "uriTemplate": template.uri_template,
        "name": template.name,
        "description": template.description,
        "mimeType": template.mime_type,
    })
}

fn resource_to_json(resource: McpResource) -> Value {
    json!({
        "contents": [{
            "uri": resource.uri,
            "mimeType": resource.mime_type,
            "text": resource.text,
        }]
    })
}

fn tool_to_json(tool: McpTool) -> Value {
    json!({
        "name": tool.name,
        "title": tool.title,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "outputSchema": tool.output_schema,
        "annotations": {
            "destructiveHint": tool.dangerous,
        }
    })
}
