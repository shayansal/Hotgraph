//! Frontier lab integration adapters for Reality Graph.

use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntegrationOperation {
    Remember,
    Recall,
    Verify,
    Explain,
    Timeline,
    EvidencePack,
    ContradictionCheck,
    Simulate,
}

impl IntegrationOperation {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Remember,
            Self::Recall,
            Self::Verify,
            Self::Explain,
            Self::Timeline,
            Self::EvidencePack,
            Self::ContradictionCheck,
            Self::Simulate,
        ]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Recall => "recall",
            Self::Verify => "verify",
            Self::Explain => "explain",
            Self::Timeline => "timeline",
            Self::EvidencePack => "evidence_pack",
            Self::ContradictionCheck => "contradiction_check",
            Self::Simulate => "simulate",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Remember => "Remember",
            Self::Recall => "Recall",
            Self::Verify => "Verify",
            Self::Explain => "Explain",
            Self::Timeline => "Timeline",
            Self::EvidencePack => "Evidence Pack",
            Self::ContradictionCheck => "Contradiction Check",
            Self::Simulate => "Simulate",
        }
    }

    fn default_description(self) -> &'static str {
        match self {
            Self::Remember => "Store source-backed memory or evidence in Reality Graph.",
            Self::Recall => "Retrieve relevant source-backed memory for an agent task.",
            Self::Verify => "Check a claim against temporal graph evidence.",
            Self::Explain => "Explain a belief, memory, or answer with evidence.",
            Self::Timeline => "Return temporal state for an entity or event.",
            Self::EvidencePack => "Build model-ready context with citations and warnings.",
            Self::ContradictionCheck => "Find unresolved contradictions in graph evidence.",
            Self::Simulate => "Run a counterfactual or action simulation labeled as prediction.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdapterKind {
    McpServer,
    OpenAiTools,
    AnthropicTools,
    LangGraph,
    LlamaIndex,
    Dspy,
    LocalAgentDaemon,
    CursorCodex,
    PythonAsyncSdk,
    RustSdk,
    TypeScriptSdk,
    KubernetesService,
}

impl AdapterKind {
    pub fn slug(self) -> &'static str {
        match self {
            Self::McpServer => "rg-mcp-server",
            Self::OpenAiTools => "rg-openai-tools-adapter",
            Self::AnthropicTools => "rg-anthropic-tools-adapter",
            Self::LangGraph => "rg-langgraph-adapter",
            Self::LlamaIndex => "rg-llamaindex-adapter",
            Self::Dspy => "rg-dspy-adapter",
            Self::LocalAgentDaemon => "rg-local-agent-daemon",
            Self::CursorCodex => "rg-cursor-codex-adapter",
            Self::PythonAsyncSdk => "reality-graph-python-async-sdk",
            Self::RustSdk => "reality-graph-rust-sdk",
            Self::TypeScriptSdk => "reality-graph-typescript-sdk",
            Self::KubernetesService => "reality-graph-kubernetes-service",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::McpServer => "MCP Server",
            Self::OpenAiTools => "OpenAI Tools Adapter",
            Self::AnthropicTools => "Anthropic Tools Adapter",
            Self::LangGraph => "LangGraph Adapter",
            Self::LlamaIndex => "LlamaIndex Adapter",
            Self::Dspy => "DSPy Adapter",
            Self::LocalAgentDaemon => "Local Agent Daemon",
            Self::CursorCodex => "Cursor/Codex Adapter",
            Self::PythonAsyncSdk => "Python Async SDK",
            Self::RustSdk => "Rust SDK",
            Self::TypeScriptSdk => "TypeScript SDK",
            Self::KubernetesService => "Kubernetes Service",
        }
    }

    fn protocol(self) -> IntegrationProtocol {
        match self {
            Self::McpServer => IntegrationProtocol::Mcp,
            Self::OpenAiTools => IntegrationProtocol::OpenAiTools,
            Self::AnthropicTools => IntegrationProtocol::AnthropicTools,
            Self::LangGraph => IntegrationProtocol::PythonFramework,
            Self::LlamaIndex => IntegrationProtocol::PythonFramework,
            Self::Dspy => IntegrationProtocol::PythonFramework,
            Self::LocalAgentDaemon => IntegrationProtocol::Http,
            Self::CursorCodex => IntegrationProtocol::Mcp,
            Self::PythonAsyncSdk => IntegrationProtocol::PythonSdk,
            Self::RustSdk => IntegrationProtocol::RustSdk,
            Self::TypeScriptSdk => IntegrationProtocol::TypeScriptSdk,
            Self::KubernetesService => IntegrationProtocol::Kubernetes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RequiredAdapter {
    McpServer,
    OpenAiToolsAdapter,
    AnthropicToolsAdapter,
    LangGraphAdapter,
    LlamaIndexAdapter,
    DspyAdapter,
    LocalAgentDaemon,
}

impl RequiredAdapter {
    pub fn all() -> Vec<Self> {
        vec![
            Self::McpServer,
            Self::OpenAiToolsAdapter,
            Self::AnthropicToolsAdapter,
            Self::LangGraphAdapter,
            Self::LlamaIndexAdapter,
            Self::DspyAdapter,
            Self::LocalAgentDaemon,
        ]
    }

    fn kind(self) -> AdapterKind {
        match self {
            Self::McpServer => AdapterKind::McpServer,
            Self::OpenAiToolsAdapter => AdapterKind::OpenAiTools,
            Self::AnthropicToolsAdapter => AdapterKind::AnthropicTools,
            Self::LangGraphAdapter => AdapterKind::LangGraph,
            Self::LlamaIndexAdapter => AdapterKind::LlamaIndex,
            Self::DspyAdapter => AdapterKind::Dspy,
            Self::LocalAgentDaemon => AdapterKind::LocalAgentDaemon,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrationProtocol {
    Mcp,
    OpenAiTools,
    AnthropicTools,
    PythonFramework,
    PythonSdk,
    RustSdk,
    TypeScriptSdk,
    Http,
    Kubernetes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntegrationCatalog {
    adapters: Vec<AdapterSpec>,
}

impl IntegrationCatalog {
    pub fn frontier_lab() -> Self {
        let adapters = vec![
            adapter_spec(AdapterKind::McpServer, 12),
            adapter_spec(AdapterKind::OpenAiTools, 10),
            adapter_spec(AdapterKind::AnthropicTools, 10),
            adapter_spec(AdapterKind::LangGraph, 18),
            adapter_spec(AdapterKind::LlamaIndex, 18),
            adapter_spec(AdapterKind::Dspy, 20),
            adapter_spec(AdapterKind::LocalAgentDaemon, 15),
            adapter_spec(AdapterKind::CursorCodex, 12),
            adapter_spec(AdapterKind::PythonAsyncSdk, 15),
            adapter_spec(AdapterKind::RustSdk, 20),
            adapter_spec(AdapterKind::TypeScriptSdk, 18),
            adapter_spec(AdapterKind::KubernetesService, 30),
        ];
        Self { adapters }
    }

    pub fn adapters(&self) -> &[AdapterSpec] {
        &self.adapters
    }

    pub fn adapter(&self, kind: AdapterKind) -> Option<&AdapterSpec> {
        self.adapters.iter().find(|adapter| adapter.kind == kind)
    }

    pub fn has_required_adapter(&self, required: RequiredAdapter) -> bool {
        self.adapter(required.kind()).is_some()
    }

    pub fn adoption_readiness(&self) -> AdoptionReadiness {
        let all_required_present = RequiredAdapter::all()
            .into_iter()
            .all(|required| self.has_required_adapter(required));
        let all_operations_exposed = self.adapters.iter().all(|adapter| {
            IntegrationOperation::all()
                .into_iter()
                .all(|operation| adapter.operation(operation).is_some())
        });
        let under_30_minutes = self
            .adapters
            .iter()
            .all(|adapter| adapter.estimated_connect_minutes <= 30);
        AdoptionReadiness {
            all_required_present,
            all_operations_exposed,
            under_30_minutes,
            connect_minutes_max: self
                .adapters
                .iter()
                .map(|adapter| adapter.estimated_connect_minutes)
                .max()
                .unwrap_or(0),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdoptionReadiness {
    pub all_required_present: bool,
    pub all_operations_exposed: bool,
    pub under_30_minutes: bool,
    pub connect_minutes_max: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdapterSpec {
    pub kind: AdapterKind,
    pub crate_name: String,
    pub protocol: IntegrationProtocol,
    pub base_url_env: Option<String>,
    pub operations: Vec<OperationSpec>,
    pub quickstart: QuickstartRecipe,
    pub estimated_connect_minutes: u8,
}

impl AdapterSpec {
    pub fn operation(&self, operation: IntegrationOperation) -> Option<&OperationSpec> {
        self.operations
            .iter()
            .find(|spec| spec.operation == operation)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperationSpec {
    pub operation: IntegrationOperation,
    pub method_name: String,
    pub route_or_tool: String,
    pub input_schema: Value,
    pub output_contract: String,
    pub write_policy: WritePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritePolicy {
    ReadOnly,
    SourceBackedWrite,
    PredictionOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickstartRecipe {
    pub title: String,
    pub steps: Vec<String>,
    pub success_check: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenAiToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpenAiToolsAdapter;

impl OpenAiToolsAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn tool_definitions(&self) -> Vec<OpenAiToolDefinition> {
        adapter_spec(AdapterKind::OpenAiTools, 10)
            .operations
            .into_iter()
            .map(|operation| OpenAiToolDefinition {
                name: operation.route_or_tool,
                description: format!(
                    "{} {}",
                    operation.operation.default_description(),
                    "Returns Reality Graph evidence-backed output."
                ),
                parameters: operation.input_schema,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnthropicToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnthropicToolsAdapter;

impl AnthropicToolsAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn tool_specs(&self) -> Vec<AnthropicToolSpec> {
        adapter_spec(AdapterKind::AnthropicTools, 10)
            .operations
            .into_iter()
            .map(|operation| AnthropicToolSpec {
                name: operation.route_or_tool,
                description: format!(
                    "{} {}",
                    operation.operation.default_description(),
                    "Returns Reality Graph evidence-backed output."
                ),
                input_schema: operation.input_schema,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkNodeSpec {
    pub node_name: String,
    pub operation: IntegrationOperation,
    pub callable: String,
    pub returns_evidence: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LangGraphAdapter;

impl LangGraphAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn node_specs(&self) -> Vec<FrameworkNodeSpec> {
        framework_nodes("reality_graph")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LlamaIndexAdapter;

impl LlamaIndexAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn tool_specs(&self) -> Vec<FrameworkNodeSpec> {
        framework_nodes("llamaindex_reality_graph")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DspyAdapter;

impl DspyAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn modules(&self) -> Vec<FrameworkNodeSpec> {
        framework_nodes("dspy_reality_graph")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAgentDaemon {
    pub bind_address: String,
    pub health_route: String,
    pub metrics_route: String,
    pub routes: Vec<DaemonRoute>,
}

impl Default for LocalAgentDaemon {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8787".to_owned(),
            health_route: "/health".to_owned(),
            metrics_route: "/metrics".to_owned(),
            routes: IntegrationOperation::all()
                .into_iter()
                .map(|operation| DaemonRoute {
                    operation,
                    method: "POST".to_owned(),
                    path: format!("/v1/{}", operation.slug().replace('_', "-")),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonRoute {
    pub operation: IntegrationOperation,
    pub method: String,
    pub path: String,
}

fn adapter_spec(kind: AdapterKind, estimated_connect_minutes: u8) -> AdapterSpec {
    AdapterSpec {
        kind,
        crate_name: kind.slug().to_owned(),
        protocol: kind.protocol(),
        base_url_env: base_url_env(kind),
        operations: IntegrationOperation::all()
            .into_iter()
            .map(|operation| operation_spec(kind, operation))
            .collect(),
        quickstart: quickstart(kind),
        estimated_connect_minutes,
    }
}

fn operation_spec(kind: AdapterKind, operation: IntegrationOperation) -> OperationSpec {
    OperationSpec {
        operation,
        method_name: method_name(kind, operation),
        route_or_tool: route_or_tool(kind, operation),
        input_schema: input_schema(operation),
        output_contract: output_contract(operation),
        write_policy: write_policy(operation),
    }
}

fn method_name(kind: AdapterKind, operation: IntegrationOperation) -> String {
    match kind.protocol() {
        IntegrationProtocol::OpenAiTools => format!("call_tool:rg_{}", operation.slug()),
        IntegrationProtocol::AnthropicTools => format!("tool_use:rg_{}", operation.slug()),
        IntegrationProtocol::Mcp => format!("tools/call:{}", operation.slug()),
        IntegrationProtocol::PythonFramework => format!("RealityGraph.{}", operation.slug()),
        IntegrationProtocol::PythonSdk => format!("await client.{}(...)", operation.slug()),
        IntegrationProtocol::RustSdk => format!("client.{}(...).await", operation.slug()),
        IntegrationProtocol::TypeScriptSdk => {
            format!("client.{}(...)", camel_case(operation.slug()))
        }
        IntegrationProtocol::Http => format!("POST {}", route_or_tool(kind, operation)),
        IntegrationProtocol::Kubernetes => format!("service:{}", route_or_tool(kind, operation)),
    }
}

fn route_or_tool(kind: AdapterKind, operation: IntegrationOperation) -> String {
    match kind.protocol() {
        IntegrationProtocol::OpenAiTools | IntegrationProtocol::AnthropicTools => {
            format!("rg_{}", operation.slug())
        }
        IntegrationProtocol::Mcp => operation.slug().to_owned(),
        IntegrationProtocol::PythonFramework => format!("reality_graph_{}", operation.slug()),
        IntegrationProtocol::PythonSdk | IntegrationProtocol::RustSdk => {
            operation.slug().to_owned()
        }
        IntegrationProtocol::TypeScriptSdk => camel_case(operation.slug()),
        IntegrationProtocol::Http | IntegrationProtocol::Kubernetes => {
            format!("/v1/{}", operation.slug().replace('_', "-"))
        }
    }
}

fn input_schema(operation: IntegrationOperation) -> Value {
    match operation {
        IntegrationOperation::Remember => object_schema(
            vec![
                ("agent_id", string_schema("Agent or user identity")),
                ("content", string_schema("Memory or observation content")),
                ("source_ids", array_schema(string_schema("Source ID"))),
                (
                    "related_entity_ids",
                    array_schema(string_schema("Related entity ID")),
                ),
                ("valid_at", string_schema("Optional valid-time anchor")),
                ("confidence", number_schema("Confidence from 0.0 to 1.0")),
            ],
            vec!["agent_id", "content", "source_ids"],
        ),
        IntegrationOperation::Recall => object_schema(
            vec![
                ("agent_id", string_schema("Agent or user identity")),
                ("task", string_schema("Task or question")),
                (
                    "entity_ids",
                    array_schema(string_schema("Optional entity filter")),
                ),
                ("limit", integer_schema("Maximum memories")),
            ],
            vec!["agent_id", "task"],
        ),
        IntegrationOperation::Verify => object_schema(
            vec![
                ("claim", string_schema("Claim to verify")),
                ("valid_at", string_schema("Optional valid time")),
                ("known_at", string_schema("Optional transaction time")),
                ("entity_ids", array_schema(string_schema("Entity filter"))),
            ],
            vec!["claim"],
        ),
        IntegrationOperation::Explain => object_schema(
            vec![
                ("question", string_schema("Explanation question")),
                ("entity_id", string_schema("Optional entity ID")),
                ("memory_id", string_schema("Optional memory ID")),
            ],
            vec!["question"],
        ),
        IntegrationOperation::Timeline => object_schema(
            vec![
                ("entity_id", string_schema("Entity or event ID")),
                ("valid_at", string_schema("Optional valid time")),
                ("known_at", string_schema("Optional transaction time")),
            ],
            vec!["entity_id"],
        ),
        IntegrationOperation::EvidencePack => object_schema(
            vec![
                ("question", string_schema("Question or task")),
                ("agent_id", string_schema("Optional agent ID")),
                (
                    "entity_ids",
                    array_schema(string_schema("Optional entity filter")),
                ),
                (
                    "max_evidence_items",
                    integer_schema("Maximum evidence items"),
                ),
            ],
            vec!["question"],
        ),
        IntegrationOperation::ContradictionCheck => object_schema(
            vec![
                ("question", string_schema("Conflict question")),
                ("entity_id", string_schema("Optional entity ID")),
                ("severity_min", string_schema("Optional severity threshold")),
            ],
            vec!["question"],
        ),
        IntegrationOperation::Simulate => object_schema(
            vec![
                ("action", string_schema("Proposed action or intervention")),
                (
                    "target_entity_ids",
                    array_schema(string_schema("Target entity ID")),
                ),
                ("horizon", string_schema("Optional simulation horizon")),
                ("max_depth", integer_schema("Maximum impact depth")),
            ],
            vec!["action"],
        ),
    }
}

fn output_contract(operation: IntegrationOperation) -> String {
    match operation {
        IntegrationOperation::Remember => {
            "stored memory id, source ids, evidence-backed write status".to_owned()
        }
        IntegrationOperation::Recall => {
            "ranked memories, source ids, evidence snippets, retrieval trace".to_owned()
        }
        IntegrationOperation::Verify => {
            "verification status, supporting evidence, contradictions, source ids".to_owned()
        }
        IntegrationOperation::Explain => {
            "explanation, belief trace, citations, evidence source ids".to_owned()
        }
        IntegrationOperation::Timeline => {
            "temporal items, valid/known times, source ids, evidence links".to_owned()
        }
        IntegrationOperation::EvidencePack => {
            "evidence pack with entities, assertions, sources, paths, contradictions".to_owned()
        }
        IntegrationOperation::ContradictionCheck => {
            "conflict sets, competing evidence, source ids, severity".to_owned()
        }
        IntegrationOperation::Simulate => {
            "prediction-only impact trace, assumptions, uncertainty, evidence references".to_owned()
        }
    }
}

fn write_policy(operation: IntegrationOperation) -> WritePolicy {
    match operation {
        IntegrationOperation::Remember => WritePolicy::SourceBackedWrite,
        IntegrationOperation::Simulate => WritePolicy::PredictionOnly,
        _ => WritePolicy::ReadOnly,
    }
}

fn quickstart(kind: AdapterKind) -> QuickstartRecipe {
    let install_step = match kind {
        AdapterKind::McpServer => "Register rg-mcp-server with the lab MCP client.",
        AdapterKind::OpenAiTools => {
            "Add rg-openai-tools-adapter tool definitions to the Responses or Agents tool list."
        }
        AdapterKind::AnthropicTools => {
            "Add rg-anthropic-tools-adapter tool specs to the Anthropic tool list."
        }
        AdapterKind::LangGraph => "Wrap the RealityGraph client as LangGraph tool nodes.",
        AdapterKind::LlamaIndex => {
            "Register RealityGraph tools with a LlamaIndex QueryEngine or agent."
        }
        AdapterKind::Dspy => "Register RealityGraph modules as DSPy tools.",
        AdapterKind::LocalAgentDaemon => "Start rg-local-agent-daemon on 127.0.0.1:8787.",
        AdapterKind::CursorCodex => "Register rg-mcp-server in Cursor or Codex tool settings.",
        AdapterKind::PythonAsyncSdk => "Install the Python async RealityGraph client package.",
        AdapterKind::RustSdk => "Add the Rust Reality Graph SDK crate to the agent experiment.",
        AdapterKind::TypeScriptSdk => "Install the TypeScript Reality Graph SDK package.",
        AdapterKind::KubernetesService => "kubectl apply -f infra/k8s/reality-graph-service.yaml.",
    };
    QuickstartRecipe {
        title: format!("Connect {}", kind.title()),
        steps: vec![
            install_step.to_owned(),
            "Set REALITY_GRAPH_URL to the local daemon, cluster service, or API endpoint."
                .to_owned(),
            "Set REALITY_GRAPH_API_KEY or configure the MCP policy gate.".to_owned(),
            "Call recall or evidence_pack with a known fixture question.".to_owned(),
        ],
        success_check:
            "The adapter returns evidence-backed output with source IDs in under one round trip."
                .to_owned(),
    }
}

fn base_url_env(kind: AdapterKind) -> Option<String> {
    matches!(
        kind.protocol(),
        IntegrationProtocol::Http
            | IntegrationProtocol::OpenAiTools
            | IntegrationProtocol::AnthropicTools
            | IntegrationProtocol::PythonFramework
            | IntegrationProtocol::PythonSdk
            | IntegrationProtocol::RustSdk
            | IntegrationProtocol::TypeScriptSdk
            | IntegrationProtocol::Kubernetes
    )
    .then(|| "REALITY_GRAPH_URL".to_owned())
}

fn framework_nodes(prefix: &str) -> Vec<FrameworkNodeSpec> {
    IntegrationOperation::all()
        .into_iter()
        .map(|operation| FrameworkNodeSpec {
            node_name: format!("{}_{}", prefix, operation.slug()),
            operation,
            callable: format!("{prefix}.{}", operation.slug()),
            returns_evidence: true,
        })
        .collect()
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

fn number_schema(description: &str) -> Value {
    json!({ "type": "number", "description": description, "minimum": 0.0, "maximum": 1.0 })
}

fn integer_schema(description: &str) -> Value {
    json!({ "type": "integer", "description": description, "minimum": 1 })
}

fn array_schema(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

fn camel_case(value: &str) -> String {
    let mut result = String::new();
    for (index, part) in value.split('_').enumerate() {
        if index == 0 {
            result.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                result.push(first.to_ascii_uppercase());
                result.extend(chars);
            }
        }
    }
    result
}
