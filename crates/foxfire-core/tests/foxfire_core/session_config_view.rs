use agent_client_protocol::v2::schema::{SessionConfigOption, SessionConfigOptionCategory};
use foxfire_acp::ReasoningEffort;
use foxfire_acp::aether::SelectOptionMeta;
use foxfire_acp::testing::{model_choice, option_group, select, thought_level_select};
use foxfire_core::session_config_view::{LocalConfigOption, LocalConfigView};

#[test]
fn control_states_survive_projection_without_default_becoming_a_capability() {
    for effort in [ReasoningEffort::Default, ReasoningEffort::Disabled] {
        let levels = [("default", "Default"), ("disabled", "Disabled"), ("low", "Low")];
        let options = [LocalConfigOption::from_acp(thought_level_select(effort.as_str(), &levels))];
        let view = LocalConfigView::new(&options);
        assert_eq!(view.reasoning_effort(), Some(effort));
        assert_eq!(view.reasoning_levels(), vec![ReasoningEffort::Disabled, ReasoningEffort::Low]);
    }
}

#[test]
fn projects_v2_wire_ids_and_tolerates_unknown_categories() {
    let option: SessionConfigOption = serde_json::from_value(serde_json::json!({
        "configId": "model",
        "name": "Model",
        "category": "future_category",
        "type": "select",
        "currentValue": "fast",
        "options": [{
            "groupId": "provider",
            "name": "Provider",
            "options": [{"value": "fast", "name": "Fast"}]
        }]
    }))
    .unwrap();
    let options = [LocalConfigOption::from_acp(option)];
    let view = LocalConfigView::new(&options);
    assert_eq!(options[0].id, "model");
    assert_eq!(options[0].category, Some(SessionConfigOptionCategory::Other("future_category".into())));
    assert_eq!(options[0].display_value(), Some("Fast"));
    assert_eq!(options[0].select().unwrap().values[0].group.as_deref(), Some("Provider"));
    assert!(view.select(&SessionConfigOptionCategory::Model).is_none(), "an option is a model by category, not id");
    assert_eq!(view.next_mode(), None);
}

#[test]
fn projects_grouped_choices_current_display_and_model_metadata() {
    let image = SelectOptionMeta { supports_image: Some(true), ..Default::default() };
    let audio = SelectOptionMeta { supports_audio: Some(true), ..Default::default() };
    let media = option_group(
        "Media",
        vec![model_choice("image", "Image", image.clone()), model_choice("audio", "Audio", audio.clone())],
    );
    let model = SessionConfigOption::select("model", "Model", "image,audio", vec![media])
        .category(SessionConfigOptionCategory::Model);
    let reasoning = thought_level_select("high", &[("low", "Low"), ("high", "High")]);
    let options: Vec<LocalConfigOption> = [model, reasoning].into_iter().map(LocalConfigOption::from_acp).collect();
    let view = LocalConfigView::new(&options);

    let model = view.select(&SessionConfigOptionCategory::Model).unwrap();
    let selected: Vec<_> = model.selected().map(|value| (value.value.as_str(), value.meta.clone())).collect();
    assert_eq!(selected, [("image", image), ("audio", audio)]);
    assert_eq!(view.current_display_name(&SessionConfigOptionCategory::Model).as_deref(), Some("Image, Audio"));
    assert!(model.values.iter().all(|value| value.group.as_deref() == Some("Media")));
    assert_eq!(view.reasoning_levels(), vec![ReasoningEffort::Low, ReasoningEffort::High]);
    assert_eq!(view.reasoning_effort(), Some(ReasoningEffort::High));
    assert_eq!(view.next_reasoning_effort(), Some(("reasoning_effort", ReasoningEffort::config_str(None))));
}

#[test]
fn finds_mode_model_and_thought_level_by_category_whatever_their_ids() {
    let options = [
        select("llm", "LLM", "fast", &[("fast", "Fast")]).category(SessionConfigOptionCategory::Model),
        select("persona", "Persona", "plan", &[("plan", "Plan"), ("code", "Code")])
            .category(SessionConfigOptionCategory::Mode),
        select("thinking", "Thinking", "low", &[("low", "Low"), ("high", "High")])
            .category(SessionConfigOptionCategory::ThoughtLevel),
        select("model", "Not a model", "x", &[("x", "X")]),
    ]
    .map(LocalConfigOption::from_acp);
    let view = LocalConfigView::new(&options);

    assert_eq!(view.current_display_name(&SessionConfigOptionCategory::Model).as_deref(), Some("Fast"));
    assert_eq!(view.current_display_name(&SessionConfigOptionCategory::Mode).as_deref(), Some("Plan"));
    assert_eq!(view.next_mode(), Some(("persona", "code")));
    assert_eq!(view.reasoning_effort(), Some(ReasoningEffort::Low));
    assert_eq!(view.next_reasoning_effort(), Some(("thinking", "high")));
}
