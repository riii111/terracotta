#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RelationSource {
    Configuration,
    State,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RelationEndpoint {
    Instance(String),
    Block(String),
}

impl RelationEndpoint {
    #[must_use]
    pub(crate) fn address(&self) -> &str {
        match self {
            Self::Instance(address) | Self::Block(address) => address,
        }
    }

    #[must_use]
    pub(crate) const fn is_instance(&self) -> bool {
        matches!(self, Self::Instance(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RelationUnresolvedReason {
    LocalValue,
    Variable,
    MissingAddress,
    AmbiguousModule,
    CyclicReference,
    InvalidConfiguration,
    InvalidState,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelationEvidence {
    pub(crate) dependent: RelationEndpoint,
    pub(crate) referenced: Option<RelationEndpoint>,
    pub(crate) source: RelationSource,
    pub(crate) unresolved: Option<RelationUnresolvedReason>,
}

impl RelationEvidence {
    #[must_use]
    pub(crate) const fn resolved(
        dependent: RelationEndpoint,
        referenced: RelationEndpoint,
        source: RelationSource,
    ) -> Self {
        Self {
            dependent,
            referenced: Some(referenced),
            source,
            unresolved: None,
        }
    }

    #[must_use]
    pub(crate) const fn unresolved(
        dependent: RelationEndpoint,
        source: RelationSource,
        reason: RelationUnresolvedReason,
    ) -> Self {
        Self {
            dependent,
            referenced: None,
            source,
            unresolved: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigurationRelationStatus {
    NotCollected,
    Available,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateRelationStatus {
    NotCollected,
    NoPriorState,
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanRelations {
    pub(crate) configuration_status: ConfigurationRelationStatus,
    pub(crate) configuration: Vec<RelationEvidence>,
    pub(crate) state_status: StateRelationStatus,
    pub(crate) state: Vec<RelationEvidence>,
}

impl PlanRelations {
    #[must_use]
    pub(crate) const fn not_collected() -> Self {
        Self {
            configuration_status: ConfigurationRelationStatus::NotCollected,
            configuration: Vec::new(),
            state_status: StateRelationStatus::NotCollected,
            state: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) const fn from_saved_plan(
        configuration_status: ConfigurationRelationStatus,
        configuration: Vec<RelationEvidence>,
        has_prior_state: bool,
    ) -> Self {
        Self {
            configuration_status,
            configuration,
            state_status: if has_prior_state {
                StateRelationStatus::NotCollected
            } else {
                StateRelationStatus::NoPriorState
            },
            state: Vec::new(),
        }
    }

    pub(crate) fn with_state(
        mut self,
        state_status: StateRelationStatus,
        state: Vec<RelationEvidence>,
    ) -> Self {
        self.state_status = state_status;
        self.state = state;
        self
    }
}
