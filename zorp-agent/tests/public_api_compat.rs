use zorp_agent::{
    ApprovalSection, AssistantMessage, CompletionTelemetry, ContentPart, Flavor, HttpModel,
    Message, Model, Provider, ToolCall, ToolsSection, VerifySection,
};

#[derive(Clone)]
struct LegacyModel;

impl Model for LegacyModel {
    fn complete(
        &self,
        _messages: &[Message],
        _tools: &[serde_json::Value],
    ) -> Result<AssistantMessage, zorp_agent::BoxErr> {
        Ok(AssistantMessage {
            content: "legacy".into(),
            tool_calls: Vec::<ToolCall>::new(),
            finish_reason: "stop".into(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(self.clone())
    }
}

#[test]
fn legacy_public_struct_literals_still_compile_and_work() {
    let model = HttpModel {
        url: "http://127.0.0.1:1/v1/chat/completions".into(),
        api_key: None,
        model: "legacy-model".into(),
        provider: Provider::OpenAiCompatible,
        max_tokens: None,
    };
    let message = LegacyModel
        .complete(&[Message::user("hello")], &[])
        .unwrap();
    // Message carries a private per-message serialization cache, so it is
    // constructed through its helpers instead of a struct literal. Field
    // reads stay public.
    let mut transcript_message = Message::assistant("legacy response");
    transcript_message.content = vec![ContentPart::Text("legacy response".into())];
    // Every field spelled out on purpose. This is what catches a field
    // added to `Flavor`, which is a breaking change for anybody building
    // one with a struct literal, and `description` arriving for the agents
    // pane is exactly the case it caught. Do not reach for
    // `..Default::default()` here: it would make this compile forever and
    // stop the test doing the one thing it is for.
    let flavor = Flavor {
        name: Some("legacy".into()),
        description: None,
        model: Some("legacy-model".into()),
        base_url: None,
        provider: None,
        max_tokens: None,
        max_steps: Some(10),
        reasoning_mode: None,
        auto_verify: None,
        auto_approve: None,
        system_prompt: None,
        system_prompt_file: None,
        tools: ToolsSection::default(),
        approval: ApprovalSection::default(),
        verify: VerifySection::default(),
    };
    let telemetry = CompletionTelemetry::default();

    assert_eq!(model.model, "legacy-model");
    assert_eq!(message.content, "legacy");
    assert_eq!(transcript_message.text(), "legacy response");
    assert_eq!(flavor.name.as_deref(), Some("legacy"));
    assert_eq!(telemetry, CompletionTelemetry::default());
}
