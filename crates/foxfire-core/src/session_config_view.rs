use acp_utils::ReasoningEffort;
use acp_utils::aether::{ConfigOptionMeta, SelectOptionMeta};
use agent_client_protocol::v2::schema::{self as acp, SessionConfigOptionCategory};

/// Client-owned configuration state projected from an ACP session schema.
///
/// ACP values are copied at the protocol boundary so optimistic edits and
/// reconciliation never mutate a protocol object borrowed by the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalConfigOption {
    pub id: String,
    pub name: String,
    pub category: Option<SessionConfigOptionCategory>,
    pub kind: LocalConfigKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalConfigKind {
    Select(LocalConfigSelect),
    Boolean { current_value: bool },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalConfigSelect {
    pub current_value: String,
    pub values: Vec<LocalConfigValue>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalConfigValue {
    pub value: String,
    pub name: String,
    pub group: Option<String>,
    pub description: Option<String>,
    pub is_disabled: bool,
    pub meta: SelectOptionMeta,
}

impl LocalConfigOption {
    pub fn from_acp(option: acp::SessionConfigOption) -> Self {
        let kind = match option.kind {
            acp::SessionConfigKind::Select(select) => LocalConfigKind::Select(LocalConfigSelect {
                current_value: select.current_value.0.to_string(),
                values: match select.options {
                    acp::SessionConfigSelectOptions::Ungrouped(options) => {
                        options.into_iter().map(|option| (None, option)).collect()
                    }
                    acp::SessionConfigSelectOptions::Grouped(groups) => groups
                        .into_iter()
                        .flat_map(|group| {
                            let name = group.name;
                            group.options.into_iter().map(move |option| (Some(name.clone()), option))
                        })
                        .collect(),
                    _ => Vec::new(),
                }
                .into_iter()
                .map(|(group, value)| LocalConfigValue {
                    value: value.value.0.to_string(),
                    name: value.name,
                    group,
                    is_disabled: value.description.as_deref().is_some_and(|text| text.starts_with("Unavailable:")),
                    description: value.description,
                    meta: SelectOptionMeta::from_meta(value.meta.as_ref()),
                })
                .collect(),
                multi_select: ConfigOptionMeta::from_meta(option.meta.as_ref()).multi_select,
            }),
            acp::SessionConfigKind::Boolean(boolean) => {
                LocalConfigKind::Boolean { current_value: boolean.current_value }
            }
            _ => LocalConfigKind::Boolean { current_value: false },
        };
        Self { id: option.config_id.0.to_string(), name: option.name, category: option.category, kind }
    }

    pub fn select(&self) -> Option<&LocalConfigSelect> {
        match &self.kind {
            LocalConfigKind::Select(select) => Some(select),
            LocalConfigKind::Boolean { .. } => None,
        }
    }

    pub fn display_value(&self) -> Option<&str> {
        let select = self.select()?;
        let current = select.values.iter().find(|value| value.value == select.current_value);
        Some(current.map_or(select.current_value.as_str(), |value| value.name.as_str()))
    }

    /// The selected value of a select option, `None` for other kinds.
    pub fn current_value(&self) -> Option<&str> {
        self.select().map(|select| select.current_value.as_str())
    }
}

impl LocalConfigSelect {
    pub fn current_values(&self) -> impl Iterator<Item = &str> {
        self.current_value.split(',').map(str::trim).filter(|value| !value.is_empty())
    }

    pub fn selected(&self) -> impl Iterator<Item = &LocalConfigValue> {
        self.current_values().filter_map(|current| self.values.iter().find(|value| value.value == current))
    }

    pub fn selected_names(&self) -> Option<String> {
        let names: Vec<_> = self.selected().map(|value| value.name.as_str()).collect();
        (!names.is_empty()).then(|| names.join(", "))
    }
}

/// A pure view over client-owned configuration state.
pub struct LocalConfigView<'a> {
    options: &'a [LocalConfigOption],
}

impl<'a> LocalConfigView<'a> {
    pub fn new(options: &'a [LocalConfigOption]) -> Self {
        Self { options }
    }

    /// The first select option the agent put in `category`, whatever its id.
    pub fn option(&self, category: &SessionConfigOptionCategory) -> Option<&'a LocalConfigOption> {
        self.options.iter().find(|option| option.category.as_ref() == Some(category) && option.select().is_some())
    }

    pub fn select(&self, category: &SessionConfigOptionCategory) -> Option<&'a LocalConfigSelect> {
        self.option(category).and_then(LocalConfigOption::select)
    }

    pub fn current_display_name(&self, category: &SessionConfigOptionCategory) -> Option<String> {
        self.select(category)?.selected_names()
    }

    pub fn next_mode(&self) -> Option<(&'a str, &'a str)> {
        let option = self.option(&SessionConfigOptionCategory::Mode)?;
        let select = option.select()?;
        let current = select.values.iter().position(|value| value.value == select.current_value).unwrap_or(0);
        let next = select.values.get((current + 1) % select.values.len().max(1))?;
        Some((option.id.as_str(), next.value.as_str()))
    }

    /// The id of the thought level option, which reasoning effort changes are sent to.
    pub fn thought_level_id(&self) -> Option<&'a str> {
        self.option(&SessionConfigOptionCategory::ThoughtLevel).map(|option| option.id.as_str())
    }

    pub fn next_reasoning_effort(&self) -> Option<(&'a str, &'a str)> {
        let levels = self.reasoning_levels();
        if levels.is_empty() {
            return None;
        }
        let next = ReasoningEffort::cycle_within(self.reasoning_effort(), &levels);
        Some((self.thought_level_id()?, ReasoningEffort::config_str(next)))
    }

    /// The thought levels the agent offers that read as reasoning efforts.
    pub fn reasoning_levels(&self) -> Vec<ReasoningEffort> {
        self.flattened_options(&SessionConfigOptionCategory::ThoughtLevel)
            .iter()
            .filter_map(|option| option.value.parse().ok())
            .filter(|effort| *effort != ReasoningEffort::Default)
            .collect()
    }

    pub fn reasoning_effort(&self) -> Option<ReasoningEffort> {
        ReasoningEffort::parse(&self.select(&SessionConfigOptionCategory::ThoughtLevel)?.current_value).unwrap_or(None)
    }

    pub(crate) fn selected_model_metadata(&self) -> Vec<SelectOptionMeta> {
        self.select(&SessionConfigOptionCategory::Model)
            .map_or_default(|select| select.selected().map(|value| value.meta.clone()).collect())
    }

    fn flattened_options(&self, category: &SessionConfigOptionCategory) -> &'a [LocalConfigValue] {
        self.select(category).map_or(&[], |select| &select.values)
    }
}
