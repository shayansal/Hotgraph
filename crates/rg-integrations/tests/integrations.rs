use rg_integrations::{
    AdapterKind, AnthropicToolsAdapter, IntegrationCatalog, IntegrationOperation, LangGraphAdapter,
    LocalAgentDaemon, OpenAiToolsAdapter, RequiredAdapter,
};

#[test]
fn catalog_contains_required_frontier_lab_adapters() {
    let catalog = IntegrationCatalog::frontier_lab();

    for required in RequiredAdapter::all() {
        assert!(
            catalog.has_required_adapter(required),
            "missing required adapter {required:?}"
        );
    }

    for kind in [
        AdapterKind::PythonAsyncSdk,
        AdapterKind::RustSdk,
        AdapterKind::TypeScriptSdk,
        AdapterKind::CursorCodex,
        AdapterKind::KubernetesService,
    ] {
        assert!(catalog.adapter(kind).is_some(), "missing {kind:?}");
    }
}

#[test]
fn every_adapter_exposes_the_ai_native_operation_set() {
    let catalog = IntegrationCatalog::frontier_lab();
    let operations = IntegrationOperation::all();

    for adapter in catalog.adapters() {
        assert!(
            adapter.estimated_connect_minutes <= 30,
            "{} should connect in under 30 minutes",
            adapter.crate_name
        );
        for operation in &operations {
            let spec = adapter.operation(*operation).unwrap_or_else(|| {
                panic!(
                    "{} missing operation {}",
                    adapter.crate_name,
                    operation.slug()
                )
            });
            assert_eq!(spec.operation, *operation);
            assert!(!spec.method_name.is_empty());
            assert!(!spec.route_or_tool.is_empty());
            assert_eq!(spec.input_schema["type"], "object");
            assert_eq!(spec.input_schema["additionalProperties"], false);
            assert!(spec.output_contract.contains("evidence"));
        }
    }
}

#[test]
fn openai_and_anthropic_tools_emit_compatible_strict_schemas() {
    let openai = OpenAiToolsAdapter::new();
    let anthropic = AnthropicToolsAdapter::new();

    let openai_tools = openai.tool_definitions();
    let anthropic_tools = anthropic.tool_specs();

    assert_eq!(openai_tools.len(), IntegrationOperation::all().len());
    assert_eq!(anthropic_tools.len(), IntegrationOperation::all().len());
    assert!(openai_tools.iter().any(|tool| tool.name == "rg_remember"));
    assert!(anthropic_tools
        .iter()
        .any(|tool| tool.name == "rg_remember"));

    for tool in openai_tools {
        assert_eq!(tool.parameters["type"], "object");
        assert_eq!(tool.parameters["additionalProperties"], false);
        assert!(tool.description.contains("Reality Graph"));
    }
    for tool in anthropic_tools {
        assert_eq!(tool.input_schema["type"], "object");
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert!(tool.description.contains("Reality Graph"));
    }
}

#[test]
fn langgraph_llamaindex_and_dspy_have_drop_in_recipes() {
    let catalog = IntegrationCatalog::frontier_lab();

    for kind in [
        AdapterKind::LangGraph,
        AdapterKind::LlamaIndex,
        AdapterKind::Dspy,
    ] {
        let adapter = catalog.adapter(kind).expect("adapter");
        assert!(adapter.quickstart.steps.len() >= 3);
        assert!(adapter
            .quickstart
            .steps
            .iter()
            .any(|step| { step.contains("REALITY_GRAPH_URL") || step.contains("RealityGraph") }));
        assert!(adapter.quickstart.success_check.contains("evidence"));
    }

    let langgraph = LangGraphAdapter::new();
    assert!(langgraph.node_specs().iter().any(|node| {
        node.node_name == "reality_graph_recall" && node.operation == IntegrationOperation::Recall
    }));
}

#[test]
fn local_daemon_exposes_http_routes_health_and_kubernetes_service_contract() {
    let daemon = LocalAgentDaemon::default();

    assert_eq!(daemon.bind_address, "127.0.0.1:8787");
    assert_eq!(daemon.health_route, "/health");
    assert_eq!(daemon.metrics_route, "/metrics");
    for operation in IntegrationOperation::all() {
        assert!(
            daemon
                .routes
                .iter()
                .any(|route| route.operation == operation && route.path.starts_with("/v1/")),
            "daemon missing route for {}",
            operation.slug()
        );
    }

    let catalog = IntegrationCatalog::frontier_lab();
    let service = catalog
        .adapter(AdapterKind::KubernetesService)
        .expect("kubernetes service");
    assert!(service
        .quickstart
        .steps
        .iter()
        .any(|step| step.contains("kubectl apply")));
}
