//! General, bounded gameplay contract and deterministic replay substrate.
//!
//! This module is deliberately separate from the original vertical-slice
//! `GameSnapshot` API. It models arbitrary (bounded) entity sets, resources,
//! abilities, objectives, and NPC policies without changing the v1 wire shape.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const GENERAL_SNAPSHOT_SCHEMA: &str = "wge.gameplay-snapshot/v2";
pub const GENERAL_TRACE_SCHEMA: &str = "wge.gameplay-trace/v2";
pub const GENERAL_RECEIPT_SCHEMA: &str = "wge.gameplay-receipt/v2";
pub const GENERAL_MAX_REPLAY_EVENTS: usize = 100_000;
pub const GENERAL_MAX_ENTITIES: usize = 256;
pub const GENERAL_MAX_LOCATIONS: usize = 1_024;
pub const GENERAL_MAX_ABILITIES: usize = 512;
pub const GENERAL_MAX_OBJECTIVES: usize = 128;
pub const GENERAL_MAX_COSTS_PER_ABILITY: usize = 32;
pub const GENERAL_MAX_REQUIREMENTS_PER_ABILITY: usize = 64;
pub const GENERAL_MAX_EFFECTS_PER_ABILITY: usize = 32;
pub const GENERAL_MAX_OBJECTIVE_REQUIREMENTS: usize = 64;
pub const GENERAL_MAX_NPC_ABILITY_PRIORITY: usize = 512;
pub const GENERAL_MAX_TAGS_PER_ENTITY: usize = 64;
pub const GENERAL_MAX_TAGS_PER_RULE: usize = 64;
pub const GENERAL_MAX_RESOURCES_PER_ENTITY: usize = 64;
pub const GENERAL_MAX_DURATION_TICKS: u32 = 1_000_000;
const MAX_IDENTIFIER_BYTES: usize = 128;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(EntityId);
string_id!(LocationId);
string_id!(AbilityId);
string_id!(ObjectiveId);
string_id!(ResourceId);
string_id!(GameplayTag);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralFailureCode {
    UnsupportedSchema,
    InvalidIdentifier,
    LimitExceeded,
    InvalidNavigation,
    InvalidEntity,
    InvalidResource,
    InvalidAbility,
    InvalidRequirement,
    InvalidEffect,
    InvalidNpcPolicy,
    InvalidObjective,
    TraceTooLong,
    UnknownEntity,
    EntityNotPlayable,
    EntityDefeated,
    MoveNotAdjacent,
    InvalidTarget,
    RequirementUnmet,
    InsufficientResource,
    AbilityCooldownActive,
    UnknownAbility,
    UnknownObjective,
    ObjectiveNotAtLocation,
    ObjectivePrerequisiteUnmet,
    GameAlreadyFinished,
    TickOverflow,
    ReplayDiverged,
    MalformedInput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralFailure {
    pub code: GeneralFailureCode,
    pub event_index: Option<usize>,
    pub subject: Option<String>,
    pub detail: String,
}

impl GeneralFailure {
    fn new(code: GeneralFailureCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            event_index: None,
            subject: None,
            detail: detail.into(),
        }
    }

    fn subject(
        code: GeneralFailureCode,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            event_index: None,
            subject: Some(subject.into()),
            detail: detail.into(),
        }
    }

    fn at_event(mut self, index: usize) -> Self {
        self.event_index = Some(index);
        self
    }
}

impl std::fmt::Display for GeneralFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.detail)?;
        if let Some(index) = self.event_index {
            write!(formatter, " (event {index})")?;
        }
        if let Some(subject) = &self.subject {
            write!(formatter, " [subject {subject}]")?;
        }
        Ok(())
    }
}

impl std::error::Error for GeneralFailure {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralGameSpec {
    pub schema_version: String,
    /// Undirected navigation graph; every edge must have its reverse.
    pub navigation: BTreeMap<LocationId, BTreeSet<LocationId>>,
    pub entities: BTreeMap<EntityId, GeneralEntitySpec>,
    pub abilities: BTreeMap<AbilityId, GeneralAbilitySpec>,
    pub objectives: BTreeMap<ObjectiveId, GeneralObjectiveSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityControl {
    Player,
    Npc { policy: NpcPolicy },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NpcPolicy {
    Passive,
    UseAbilities { priority: Vec<AbilityId> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralEntitySpec {
    pub control: EntityControl,
    pub location: LocationId,
    /// Runtime resource amounts keyed by a typed, model-defined resource name.
    pub resources: BTreeMap<ResourceId, ResourceStateSpec>,
    /// Zero means the entity is not defeated by resource depletion.
    pub vital_resource: Option<ResourceId>,
    pub tags: BTreeSet<GameplayTag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceStateSpec {
    pub current: u32,
    pub maximum: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralAbilitySpec {
    pub targeting: TargetingRule,
    pub costs: Vec<ResourceCost>,
    pub cooldown_ticks: u32,
    pub requirements: Vec<AbilityRequirement>,
    pub effects: Vec<EffectSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetingRule {
    SelfOnly,
    Entity {
        required_tags: BTreeSet<GameplayTag>,
        forbidden_tags: BTreeSet<GameplayTag>,
        max_navigation_steps: Option<u16>,
        allow_self: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostPayer {
    Actor,
    SelectedTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceCost {
    pub payer: CostPayer,
    pub resource: ResourceId,
    pub amount: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AbilityRequirement {
    ActorHasTags { tags: BTreeSet<GameplayTag> },
    ActorLacksTags { tags: BTreeSet<GameplayTag> },
    TargetHasTags { tags: BTreeSet<GameplayTag> },
    TargetLacksTags { tags: BTreeSet<GameplayTag> },
    ActorResourceAtLeast { resource: ResourceId, amount: u32 },
    TargetResourceAtLeast { resource: ResourceId, amount: u32 },
    ObjectiveSecured { objective: ObjectiveId },
    ObjectiveUnsecured { objective: ObjectiveId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSpec {
    /// Stable semantic identity used by duration and stacking.
    pub stack_key: String,
    pub recipient: EffectRecipient,
    pub operation: EffectOperation,
    /// Zero applies once on activation. Positive durations require a period and
    /// apply at each due action tick until (and including) the expiry tick.
    pub duration_ticks: u32,
    pub period_ticks: Option<u32>,
    pub stacking: StackingPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectRecipient {
    Actor,
    SelectedTarget,
    AlliesInRange { max_navigation_steps: u16 },
    EnemiesInRange { max_navigation_steps: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectOperation {
    Damage { resource: ResourceId, amount: u32 },
    Heal { resource: ResourceId, amount: u32 },
    ResourceDelta { resource: ResourceId, delta: i32 },
    SetTag { tag: GameplayTag, present: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StackingPolicy {
    Replace,
    Refresh,
    AddStacks { max_stacks: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralObjectiveSpec {
    pub location: LocationId,
    pub required_for_victory: bool,
    pub prerequisites: Vec<ObjectiveRequirement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObjectiveRequirement {
    AllEntitiesDefeatedWithTags {
        tags: BTreeSet<GameplayTag>,
    },
    EntityHasTag {
        entity: EntityId,
        tag: GameplayTag,
    },
    ResourceAtLeast {
        entity: EntityId,
        resource: ResourceId,
        amount: u32,
    },
    ObjectiveSecured {
        objective: ObjectiveId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralTrace {
    pub schema_version: String,
    pub events: Vec<GeneralInputEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeneralInputEvent {
    SelectEntity {
        entity: EntityId,
    },
    Move {
        destination: LocationId,
    },
    ActivateAbility {
        ability: AbilityId,
        target: EntityId,
    },
    InteractObjective {
        objective: ObjectiveId,
    },
    Wait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralOutcome {
    InProgress,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveStatus {
    Available,
    Secured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralRuntimeEntity {
    pub control: EntityControl,
    pub location: LocationId,
    pub resources: BTreeMap<ResourceId, ResourceValue>,
    pub vital_resource: Option<ResourceId>,
    pub tags: BTreeSet<GameplayTag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceValue {
    pub current: u32,
    pub maximum: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveEffect {
    pub source: EntityId,
    pub ability: AbilityId,
    pub operation: EffectOperation,
    pub started_at_tick: u64,
    pub expires_at_tick: u64,
    pub next_tick: u64,
    pub period_ticks: u32,
    pub stacks: u16,
    pub stacking: StackingPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralRuntimeState {
    pub tick: u64,
    pub selected_entity: EntityId,
    pub entities: BTreeMap<EntityId, GeneralRuntimeEntity>,
    pub cooldown_ready_at: BTreeMap<EntityId, BTreeMap<AbilityId, u64>>,
    pub active_effects: BTreeMap<EntityId, BTreeMap<String, ActiveEffect>>,
    pub objectives: BTreeMap<ObjectiveId, ObjectiveStatus>,
    pub outcome: GeneralOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeneralTransition {
    EntitySelected {
        entity: EntityId,
    },
    EntityMoved {
        entity: EntityId,
        from: LocationId,
        to: LocationId,
    },
    AbilityActivated {
        ability: AbilityId,
        actor: EntityId,
        target: EntityId,
        costs_paid: Vec<PaidCost>,
        effects: Vec<EffectResult>,
        ready_at_tick: u64,
    },
    ObjectiveSecured {
        actor: EntityId,
        objective: ObjectiveId,
    },
    Waited,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaidCost {
    pub payer: EntityId,
    pub resource: ResourceId,
    pub amount: u32,
    pub remaining: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectResult {
    pub stack_key: String,
    pub recipient: EntityId,
    pub operation: EffectOperation,
    pub requested_delta: Option<i64>,
    pub applied_delta: Option<i64>,
    pub resource_after: Option<u32>,
    pub tag_changed: Option<bool>,
    pub duration_ticks: u32,
    pub stacks: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NpcAction {
    Defeated {
        npc: EntityId,
    },
    NoAction {
        npc: EntityId,
    },
    UsedAbility {
        npc: EntityId,
        ability: AbilityId,
        target: EntityId,
        effects: Vec<EffectResult>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralEventReceipt {
    pub event_index: usize,
    pub tick: u64,
    pub input: GeneralInputEvent,
    pub transition: GeneralTransition,
    pub npc_actions: Vec<NpcAction>,
    pub periodic_effects: Vec<EffectResult>,
    pub resulting_state_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralReceiptBody {
    pub schema_version: String,
    pub snapshot_sha256: String,
    pub trace_sha256: String,
    pub event_count: usize,
    pub outcome: GeneralOutcome,
    pub final_state_sha256: String,
    pub final_state: GeneralRuntimeState,
    pub events: Vec<GeneralEventReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralGameplayReceipt {
    pub body: GeneralReceiptBody,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralReplayExpectation {
    pub snapshot_sha256: String,
    pub trace_sha256: String,
    pub receipt_sha256: String,
}

impl GeneralGameplayReceipt {
    pub fn expectation(&self) -> GeneralReplayExpectation {
        GeneralReplayExpectation {
            snapshot_sha256: self.body.snapshot_sha256.clone(),
            trace_sha256: self.body.trace_sha256.clone(),
            receipt_sha256: self.receipt_sha256.clone(),
        }
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, GeneralFailure> {
        serde_json::to_vec(self).map_err(|error| {
            GeneralFailure::new(
                GeneralFailureCode::MalformedInput,
                format!("receipt serialization failed: {error}"),
            )
        })
    }
}

pub fn validate_general_snapshot(snapshot: &GeneralGameSpec) -> Result<(), GeneralFailure> {
    if snapshot.schema_version != GENERAL_SNAPSHOT_SCHEMA {
        return Err(GeneralFailure::new(
            GeneralFailureCode::UnsupportedSchema,
            format!("expected snapshot schema {GENERAL_SNAPSHOT_SCHEMA:?}"),
        ));
    }
    check_limit("entity", snapshot.entities.len(), GENERAL_MAX_ENTITIES)?;
    check_limit("location", snapshot.navigation.len(), GENERAL_MAX_LOCATIONS)?;
    check_limit("ability", snapshot.abilities.len(), GENERAL_MAX_ABILITIES)?;
    check_limit(
        "objective",
        snapshot.objectives.len(),
        GENERAL_MAX_OBJECTIVES,
    )?;
    if snapshot.entities.is_empty() {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidEntity,
            "at least one entity is required",
        ));
    }
    if snapshot.navigation.is_empty() {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidNavigation,
            "at least one navigation location is required",
        ));
    }
    if snapshot.objectives.is_empty()
        || !snapshot
            .objectives
            .values()
            .any(|objective| objective.required_for_victory)
    {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidObjective,
            "at least one objective required for victory is required",
        ));
    }

    for location in snapshot.navigation.keys() {
        validate_identifier(location.as_str(), "navigation location")?;
    }
    for (location, neighbors) in &snapshot.navigation {
        for neighbor in neighbors {
            if location == neighbor
                || !snapshot
                    .navigation
                    .get(neighbor)
                    .is_some_and(|reverse| reverse.contains(location))
            {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidNavigation,
                    location.as_str(),
                    "navigation edges must be non-self and have a reverse edge",
                ));
            }
        }
    }

    let player_count = snapshot
        .entities
        .values()
        .filter(|entity| matches!(entity.control, EntityControl::Player))
        .count();
    if player_count == 0 {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidEntity,
            "at least one player-controlled entity is required",
        ));
    }

    for (id, entity) in &snapshot.entities {
        validate_identifier(id.as_str(), "entity id")?;
        validate_identifier(entity.location.as_str(), "entity location")?;
        if !snapshot.navigation.contains_key(&entity.location) {
            return Err(GeneralFailure::subject(
                GeneralFailureCode::InvalidEntity,
                id.as_str(),
                "entity location is absent from navigation",
            ));
        }
        check_limit("entity tag", entity.tags.len(), GENERAL_MAX_TAGS_PER_ENTITY)?;
        check_limit(
            "entity resource",
            entity.resources.len(),
            GENERAL_MAX_RESOURCES_PER_ENTITY,
        )?;
        for tag in &entity.tags {
            validate_identifier(tag.as_str(), "gameplay tag")?;
        }
        for (resource, value) in &entity.resources {
            validate_identifier(resource.as_str(), "resource id")?;
            if value.maximum == 0 || value.current > value.maximum {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidResource,
                    format!("{}:{}", id.as_str(), resource.as_str()),
                    "resource maximum must be positive and current must not exceed maximum",
                ));
            }
        }
        if let Some(vital) = &entity.vital_resource {
            if !entity.resources.contains_key(vital) {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidResource,
                    id.as_str(),
                    format!("vital resource {:?} is not declared", vital.as_str()),
                ));
            }
            if entity.resources[vital].current == 0 {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidEntity,
                    id.as_str(),
                    "entities must begin with a positive vital resource",
                ));
            }
        }
        if let EntityControl::Npc {
            policy: NpcPolicy::UseAbilities { priority },
        } = &entity.control
        {
            if priority.is_empty() {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidNpcPolicy,
                    id.as_str(),
                    "an ability-using NPC must declare a nonempty priority list",
                ));
            }
            check_limit(
                "NPC ability priority",
                priority.len(),
                GENERAL_MAX_NPC_ABILITY_PRIORITY,
            )?;
            let mut unique = BTreeSet::new();
            for ability in priority {
                if !snapshot.abilities.contains_key(ability) || !unique.insert(ability) {
                    return Err(GeneralFailure::subject(
                        GeneralFailureCode::InvalidNpcPolicy,
                        id.as_str(),
                        format!(
                            "NPC ability {:?} is undeclared or duplicated",
                            ability.as_str()
                        ),
                    ));
                }
            }
        }
    }

    let mut stack_definitions = BTreeMap::<String, (EffectOperation, u32, StackingPolicy)>::new();
    for (ability_id, ability) in &snapshot.abilities {
        validate_identifier(ability_id.as_str(), "ability id")?;
        if ability.cooldown_ticks == 0 {
            return Err(GeneralFailure::subject(
                GeneralFailureCode::InvalidAbility,
                ability_id.as_str(),
                "cooldown must be at least one tick",
            ));
        }
        check_limit(
            "ability effect",
            ability.effects.len(),
            GENERAL_MAX_EFFECTS_PER_ABILITY,
        )?;
        check_limit(
            "ability cost",
            ability.costs.len(),
            GENERAL_MAX_COSTS_PER_ABILITY,
        )?;
        check_limit(
            "ability requirement",
            ability.requirements.len(),
            GENERAL_MAX_REQUIREMENTS_PER_ABILITY,
        )?;
        if ability.effects.is_empty() {
            return Err(GeneralFailure::subject(
                GeneralFailureCode::InvalidAbility,
                ability_id.as_str(),
                "ability must have at least one effect",
            ));
        }
        validate_targeting(&ability.targeting)?;
        for cost in &ability.costs {
            validate_identifier(cost.resource.as_str(), "cost resource")?;
            if cost.amount == 0 {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidAbility,
                    ability_id.as_str(),
                    "resource costs must be positive",
                ));
            }
        }
        for requirement in &ability.requirements {
            validate_requirement(requirement, snapshot)?;
        }
        for effect in &ability.effects {
            validate_identifier(&effect.stack_key, "effect stack key")?;
            validate_effect(effect)?;
            if effect.duration_ticks > 0 {
                let period = effect.period_ticks.expect("validated periodic effect");
                let signature = (effect.operation.clone(), period, effect.stacking.clone());
                if let Some(existing) = stack_definitions.get(&effect.stack_key) {
                    if existing != &signature {
                        return Err(GeneralFailure::subject(
                            GeneralFailureCode::InvalidEffect,
                            effect.stack_key.clone(),
                            "a stack key must keep the same operation, period, and stacking policy across abilities",
                        ));
                    }
                } else {
                    stack_definitions.insert(effect.stack_key.clone(), signature);
                }
            }
        }
        validate_ability_bindings(snapshot, ability_id, ability)?;
    }

    for (objective_id, objective) in &snapshot.objectives {
        validate_identifier(objective_id.as_str(), "objective id")?;
        validate_identifier(objective.location.as_str(), "objective location")?;
        if !snapshot.navigation.contains_key(&objective.location) {
            return Err(GeneralFailure::subject(
                GeneralFailureCode::InvalidObjective,
                objective_id.as_str(),
                "objective location is absent from navigation",
            ));
        }
        check_limit(
            "objective prerequisite",
            objective.prerequisites.len(),
            GENERAL_MAX_OBJECTIVE_REQUIREMENTS,
        )?;
        for requirement in &objective.prerequisites {
            validate_objective_requirement(requirement, objective_id, snapshot)?;
        }
    }
    validate_objective_acyclic(snapshot)?;
    Ok(())
}

pub fn run_general_replay(
    snapshot: &GeneralGameSpec,
    trace: &GeneralTrace,
) -> Result<GeneralGameplayReceipt, GeneralFailure> {
    validate_general_snapshot(snapshot)?;
    if trace.schema_version != GENERAL_TRACE_SCHEMA {
        return Err(GeneralFailure::new(
            GeneralFailureCode::UnsupportedSchema,
            format!("expected trace schema {GENERAL_TRACE_SCHEMA:?}"),
        ));
    }
    if trace.events.len() > GENERAL_MAX_REPLAY_EVENTS {
        return Err(GeneralFailure::new(
            GeneralFailureCode::TraceTooLong,
            format!(
                "trace has {} events; maximum is {GENERAL_MAX_REPLAY_EVENTS}",
                trace.events.len()
            ),
        ));
    }

    let snapshot_sha256 = sha256_json(snapshot)?;
    let trace_sha256 = sha256_json(trace)?;
    let mut state = initial_state(snapshot)?;
    let mut receipts = Vec::with_capacity(trace.events.len());

    for (index, input) in trace.events.iter().enumerate() {
        if state.outcome != GeneralOutcome::InProgress {
            return Err(GeneralFailure::new(
                GeneralFailureCode::GameAlreadyFinished,
                "an input follows a terminal game outcome",
            )
            .at_event(index));
        }
        let tick = state.tick.checked_add(1).ok_or_else(|| {
            GeneralFailure::new(GeneralFailureCode::TickOverflow, "simulation tick overflow")
                .at_event(index)
        })?;
        let mut next = state.clone();
        let transition = apply_player_input(snapshot, &mut next, input, tick)
            .map_err(|failure| failure.at_event(index))?;
        next.tick = tick;
        let mut npc_actions = Vec::new();
        if !all_required_objectives_secured(snapshot, &next) && any_player_alive(snapshot, &next) {
            let npc_ids: Vec<_> = snapshot
                .entities
                .iter()
                .filter_map(|(id, entity)| {
                    matches!(entity.control, EntityControl::Npc { .. }).then_some(id.clone())
                })
                .collect();
            for npc_id in npc_ids {
                let action = apply_npc_policy(snapshot, &mut next, &npc_id, tick)
                    .map_err(|failure| failure.at_event(index))?;
                npc_actions.push(action);
            }
        }
        let periodic_effects =
            advance_timed_effects(&mut next, tick).map_err(|failure| failure.at_event(index))?;
        if all_required_objectives_secured(snapshot, &next) {
            next.outcome = GeneralOutcome::Won;
        } else if !any_player_alive(snapshot, &next) {
            next.outcome = GeneralOutcome::Lost;
        }
        let resulting_state_sha256 = sha256_json(&next)?;
        receipts.push(GeneralEventReceipt {
            event_index: index,
            tick,
            input: input.clone(),
            transition,
            npc_actions,
            periodic_effects,
            resulting_state_sha256,
        });
        state = next;
    }

    let body = GeneralReceiptBody {
        schema_version: GENERAL_RECEIPT_SCHEMA.to_owned(),
        snapshot_sha256,
        trace_sha256,
        event_count: receipts.len(),
        outcome: state.outcome,
        final_state_sha256: sha256_json(&state)?,
        final_state: state,
        events: receipts,
    };
    let receipt_sha256 = sha256_json(&body)?;
    Ok(GeneralGameplayReceipt {
        body,
        receipt_sha256,
    })
}

pub fn verify_general_replay(
    snapshot: &GeneralGameSpec,
    trace: &GeneralTrace,
    expected: &GeneralReplayExpectation,
) -> Result<GeneralGameplayReceipt, GeneralFailure> {
    let observed = run_general_replay(snapshot, trace)?;
    let actual = observed.expectation();
    if &actual != expected {
        let field = if actual.snapshot_sha256 != expected.snapshot_sha256 {
            "snapshot_sha256"
        } else if actual.trace_sha256 != expected.trace_sha256 {
            "trace_sha256"
        } else {
            "receipt_sha256"
        };
        return Err(GeneralFailure::subject(
            GeneralFailureCode::ReplayDiverged,
            field,
            "replayed content digest differs from the expected evidence",
        ));
    }
    Ok(observed)
}

/// Recomputes all simulation evidence and compares the full receipt. Rehashing
/// a forged, self-consistent-looking receipt does not make it authoritative.
pub fn verify_general_receipt(
    snapshot: &GeneralGameSpec,
    trace: &GeneralTrace,
    claimed: &GeneralGameplayReceipt,
) -> Result<(), GeneralFailure> {
    let observed = run_general_replay(snapshot, trace)?;
    if &observed != claimed {
        return Err(GeneralFailure::new(
            GeneralFailureCode::ReplayDiverged,
            "claimed receipt does not equal the independently replayed receipt",
        ));
    }
    Ok(())
}

fn initial_state(snapshot: &GeneralGameSpec) -> Result<GeneralRuntimeState, GeneralFailure> {
    let selected_entity = snapshot
        .entities
        .iter()
        .find_map(|(id, entity)| {
            matches!(entity.control, EntityControl::Player).then_some(id.clone())
        })
        .ok_or_else(|| {
            GeneralFailure::new(GeneralFailureCode::InvalidEntity, "no player entity")
        })?;
    let entities = snapshot
        .entities
        .iter()
        .map(|(id, entity)| {
            let resources = entity
                .resources
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        ResourceValue {
                            current: value.current,
                            maximum: value.maximum,
                        },
                    )
                })
                .collect();
            (
                id.clone(),
                GeneralRuntimeEntity {
                    control: entity.control.clone(),
                    location: entity.location.clone(),
                    resources,
                    vital_resource: entity.vital_resource.clone(),
                    tags: entity.tags.clone(),
                },
            )
        })
        .collect();
    Ok(GeneralRuntimeState {
        tick: 0,
        selected_entity,
        entities,
        cooldown_ready_at: BTreeMap::new(),
        active_effects: BTreeMap::new(),
        objectives: snapshot
            .objectives
            .keys()
            .map(|id| (id.clone(), ObjectiveStatus::Available))
            .collect(),
        outcome: GeneralOutcome::InProgress,
    })
}

fn apply_player_input(
    snapshot: &GeneralGameSpec,
    state: &mut GeneralRuntimeState,
    input: &GeneralInputEvent,
    tick: u64,
) -> Result<GeneralTransition, GeneralFailure> {
    match input {
        GeneralInputEvent::SelectEntity { entity } => {
            require_living_player(state, entity)?;
            state.selected_entity = entity.clone();
            Ok(GeneralTransition::EntitySelected {
                entity: entity.clone(),
            })
        }
        GeneralInputEvent::Move { destination } => {
            let actor = state.selected_entity.clone();
            require_living_player(state, &actor)?;
            let runtime = state.entities.get_mut(&actor).ok_or_else(|| {
                GeneralFailure::subject(
                    GeneralFailureCode::UnknownEntity,
                    actor.as_str(),
                    "actor has no runtime state",
                )
            })?;
            if !snapshot
                .navigation
                .get(&runtime.location)
                .is_some_and(|neighbors| neighbors.contains(destination))
            {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::MoveNotAdjacent,
                    destination.as_str(),
                    "destination is not adjacent to the selected entity",
                ));
            }
            let from = runtime.location.clone();
            runtime.location = destination.clone();
            Ok(GeneralTransition::EntityMoved {
                entity: actor,
                from,
                to: destination.clone(),
            })
        }
        GeneralInputEvent::ActivateAbility { ability, target } => {
            let actor = state.selected_entity.clone();
            activate_ability(snapshot, state, &actor, ability, target, tick)
        }
        GeneralInputEvent::InteractObjective { objective } => {
            let actor = state.selected_entity.clone();
            require_living_player(state, &actor)?;
            let objective_spec = snapshot.objectives.get(objective).ok_or_else(|| {
                GeneralFailure::subject(
                    GeneralFailureCode::UnknownObjective,
                    objective.as_str(),
                    "objective is not declared",
                )
            })?;
            if state.entities[&actor].location != objective_spec.location {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::ObjectiveNotAtLocation,
                    objective.as_str(),
                    "selected entity is not at the objective location",
                ));
            }
            if !objective_spec
                .prerequisites
                .iter()
                .all(|requirement| objective_requirement_met(requirement, snapshot, state))
            {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::ObjectivePrerequisiteUnmet,
                    objective.as_str(),
                    "one or more objective prerequisites are unmet",
                ));
            }
            if state.objectives.get(objective) == Some(&ObjectiveStatus::Secured) {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::ObjectivePrerequisiteUnmet,
                    objective.as_str(),
                    "objective has already been secured",
                ));
            }
            state
                .objectives
                .insert(objective.clone(), ObjectiveStatus::Secured);
            Ok(GeneralTransition::ObjectiveSecured {
                actor,
                objective: objective.clone(),
            })
        }
        GeneralInputEvent::Wait => Ok(GeneralTransition::Waited),
    }
}

fn activate_ability(
    snapshot: &GeneralGameSpec,
    state: &mut GeneralRuntimeState,
    actor: &EntityId,
    ability_id: &AbilityId,
    target: &EntityId,
    tick: u64,
) -> Result<GeneralTransition, GeneralFailure> {
    if !state.entities.contains_key(actor) {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::UnknownEntity,
            actor.as_str(),
            "ability actor is undeclared",
        ));
    }
    if !entity_alive(state, actor) {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::EntityDefeated,
            actor.as_str(),
            "defeated entity cannot activate an ability",
        ));
    }
    let ability = snapshot.abilities.get(ability_id).ok_or_else(|| {
        GeneralFailure::subject(
            GeneralFailureCode::UnknownAbility,
            ability_id.as_str(),
            "ability is not declared",
        )
    })?;
    if !target_is_valid(snapshot, state, actor, target, &ability.targeting) {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::InvalidTarget,
            target.as_str(),
            "target violates the ability targeting rule",
        ));
    }
    for requirement in &ability.requirements {
        if !requirement_met(requirement, snapshot, state, actor, target) {
            return Err(GeneralFailure::subject(
                GeneralFailureCode::RequirementUnmet,
                ability_id.as_str(),
                "ability requirement is not satisfied",
            ));
        }
    }
    let ready_at = state
        .cooldown_ready_at
        .get(actor)
        .and_then(|entries| entries.get(ability_id))
        .copied()
        .unwrap_or(0);
    if tick < ready_at {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::AbilityCooldownActive,
            ability_id.as_str(),
            format!("ability is ready at tick {ready_at}; current tick is {tick}"),
        ));
    }
    if let Some((payer, resource, required, available)) =
        cost_shortfall(state, actor, target, &ability.costs)
    {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::InsufficientResource,
            payer.as_str(),
            format!(
                "resource {:?} costs {required} in total; entity has {available}",
                resource.as_str()
            ),
        ));
    }
    for effect in &ability.effects {
        let Some(resource) = effect_resource(&effect.operation) else {
            continue;
        };
        for recipient in effect_recipients(snapshot, state, actor, target, &effect.recipient) {
            if !state.entities[&recipient].resources.contains_key(resource) {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidResource,
                    recipient.as_str(),
                    format!(
                        "effect {:?} requires undeclared recipient resource {:?}",
                        effect.stack_key,
                        resource.as_str()
                    ),
                ));
            }
        }
    }

    let mut paid_costs = Vec::with_capacity(ability.costs.len());
    for cost in &ability.costs {
        let payer = match cost.payer {
            CostPayer::Actor => actor,
            CostPayer::SelectedTarget => target,
        };
        let resource = state
            .entities
            .get_mut(payer)
            .unwrap()
            .resources
            .get_mut(&cost.resource)
            .unwrap();
        resource.current -= cost.amount;
        paid_costs.push(PaidCost {
            payer: payer.clone(),
            resource: cost.resource.clone(),
            amount: cost.amount,
            remaining: resource.current,
        });
    }

    let mut results = Vec::new();
    for effect in &ability.effects {
        let recipients = effect_recipients(snapshot, state, actor, target, &effect.recipient);
        for recipient in recipients {
            if effect.duration_ticks == 0 {
                results.push(apply_instant_effect(state, effect, &recipient)?);
            } else {
                register_timed_effect(state, actor, ability_id, effect, &recipient, tick)?;
                let stacks = active_stack_count(state, &recipient, &effect.stack_key);
                results.push(EffectResult {
                    stack_key: effect.stack_key.clone(),
                    recipient,
                    operation: effect.operation.clone(),
                    requested_delta: None,
                    applied_delta: None,
                    resource_after: None,
                    tag_changed: None,
                    duration_ticks: effect.duration_ticks,
                    stacks,
                });
            }
        }
    }
    let ready_at_tick = tick
        .checked_add(u64::from(ability.cooldown_ticks))
        .ok_or_else(|| {
            GeneralFailure::new(
                GeneralFailureCode::TickOverflow,
                "ability cooldown tick overflow",
            )
        })?;
    state
        .cooldown_ready_at
        .entry(actor.clone())
        .or_default()
        .insert(ability_id.clone(), ready_at_tick);
    Ok(GeneralTransition::AbilityActivated {
        ability: ability_id.clone(),
        actor: actor.clone(),
        target: target.clone(),
        costs_paid: paid_costs,
        effects: results,
        ready_at_tick,
    })
}

fn apply_instant_effect(
    state: &mut GeneralRuntimeState,
    effect: &EffectSpec,
    recipient: &EntityId,
) -> Result<EffectResult, GeneralFailure> {
    let runtime = state.entities.get_mut(recipient).ok_or_else(|| {
        GeneralFailure::subject(
            GeneralFailureCode::UnknownEntity,
            recipient.as_str(),
            "effect recipient is absent",
        )
    })?;
    let mut requested_delta = None;
    let mut applied_delta = None;
    let mut resource_after = None;
    let mut tag_changed = None;
    match &effect.operation {
        EffectOperation::Damage { resource, amount } => {
            let value = runtime.resources.get_mut(resource).ok_or_else(|| {
                GeneralFailure::subject(
                    GeneralFailureCode::InvalidResource,
                    recipient.as_str(),
                    format!("effect resource {:?} is absent", resource.as_str()),
                )
            })?;
            let applied = (*amount).min(value.current);
            value.current -= applied;
            requested_delta = Some(-i64::from(*amount));
            applied_delta = Some(-i64::from(applied));
            resource_after = Some(value.current);
        }
        EffectOperation::Heal { resource, amount } => {
            let value = runtime.resources.get_mut(resource).ok_or_else(|| {
                GeneralFailure::subject(
                    GeneralFailureCode::InvalidResource,
                    recipient.as_str(),
                    format!("effect resource {:?} is absent", resource.as_str()),
                )
            })?;
            let applied = (*amount).min(value.maximum - value.current);
            value.current += applied;
            requested_delta = Some(i64::from(*amount));
            applied_delta = Some(i64::from(applied));
            resource_after = Some(value.current);
        }
        EffectOperation::ResourceDelta { resource, delta } => {
            let value = runtime.resources.get_mut(resource).ok_or_else(|| {
                GeneralFailure::subject(
                    GeneralFailureCode::InvalidResource,
                    recipient.as_str(),
                    format!("effect resource {:?} is absent", resource.as_str()),
                )
            })?;
            let requested = i64::from(*delta);
            let next = (i64::from(value.current) + requested).clamp(0, i64::from(value.maximum));
            let applied = next - i64::from(value.current);
            value.current = next as u32;
            requested_delta = Some(requested);
            applied_delta = Some(applied);
            resource_after = Some(value.current);
        }
        EffectOperation::SetTag { tag, present } => {
            let changed = if *present {
                runtime.tags.insert(tag.clone())
            } else {
                runtime.tags.remove(tag)
            };
            tag_changed = Some(changed);
        }
    }
    Ok(EffectResult {
        stack_key: effect.stack_key.clone(),
        recipient: recipient.clone(),
        operation: effect.operation.clone(),
        requested_delta,
        applied_delta,
        resource_after,
        tag_changed,
        duration_ticks: 0,
        stacks: 1,
    })
}

fn register_timed_effect(
    state: &mut GeneralRuntimeState,
    source: &EntityId,
    ability: &AbilityId,
    effect: &EffectSpec,
    recipient: &EntityId,
    tick: u64,
) -> Result<(), GeneralFailure> {
    let period = effect.period_ticks.expect("timed effects validated");
    let expires_at_tick = tick
        .checked_add(u64::from(effect.duration_ticks))
        .ok_or_else(|| {
            GeneralFailure::new(GeneralFailureCode::TickOverflow, "effect expiry overflow")
        })?;
    let next_tick = tick.checked_add(u64::from(period)).ok_or_else(|| {
        GeneralFailure::new(GeneralFailureCode::TickOverflow, "effect schedule overflow")
    })?;
    let effects = state.active_effects.entry(recipient.clone()).or_default();
    match effects.get_mut(&effect.stack_key) {
        Some(active) => match effect.stacking {
            StackingPolicy::Replace => {
                *active = ActiveEffect {
                    source: source.clone(),
                    ability: ability.clone(),
                    operation: effect.operation.clone(),
                    started_at_tick: tick,
                    expires_at_tick,
                    next_tick,
                    period_ticks: period,
                    stacks: 1,
                    stacking: effect.stacking.clone(),
                };
            }
            StackingPolicy::Refresh => {
                active.source = source.clone();
                active.ability = ability.clone();
                active.started_at_tick = tick;
                active.expires_at_tick = expires_at_tick;
                active.next_tick = next_tick;
            }
            StackingPolicy::AddStacks { max_stacks } => {
                active.stacks = active.stacks.saturating_add(1).min(max_stacks);
                active.source = source.clone();
                active.ability = ability.clone();
                active.started_at_tick = tick;
                active.expires_at_tick = expires_at_tick;
                active.next_tick = next_tick;
            }
        },
        None => {
            effects.insert(
                effect.stack_key.clone(),
                ActiveEffect {
                    source: source.clone(),
                    ability: ability.clone(),
                    operation: effect.operation.clone(),
                    started_at_tick: tick,
                    expires_at_tick,
                    next_tick,
                    period_ticks: period,
                    stacks: 1,
                    stacking: effect.stacking.clone(),
                },
            );
        }
    }
    Ok(())
}

fn active_stack_count(state: &GeneralRuntimeState, recipient: &EntityId, key: &str) -> u16 {
    state
        .active_effects
        .get(recipient)
        .and_then(|effects| effects.get(key))
        .map_or(0, |effect| effect.stacks)
}

fn advance_timed_effects(
    state: &mut GeneralRuntimeState,
    tick: u64,
) -> Result<Vec<EffectResult>, GeneralFailure> {
    let mut due = Vec::<(EntityId, String, ActiveEffect)>::new();
    let mut expired = Vec::<(EntityId, String)>::new();
    let mut results = Vec::new();
    for (target, effects) in &state.active_effects {
        for (key, effect) in effects {
            if tick > effect.expires_at_tick {
                expired.push((target.clone(), key.clone()));
            } else if effect.next_tick <= tick {
                due.push((target.clone(), key.clone(), effect.clone()));
            }
        }
    }
    for (target, key) in expired {
        if let Some(effects) = state.active_effects.get_mut(&target) {
            effects.remove(&key);
        }
    }
    for (target, key, effect) in due {
        let stacks = i64::from(effect.stacks);
        let result = EffectSpec {
            stack_key: key.clone(),
            recipient: EffectRecipient::SelectedTarget,
            operation: scale_operation(&effect.operation, stacks)?,
            duration_ticks: 0,
            period_ticks: None,
            stacking: StackingPolicy::Replace,
        };
        let mut applied = apply_instant_effect(state, &result, &target)?;
        applied.operation = effect.operation.clone();
        applied.duration_ticks = u32::try_from(effect.expires_at_tick - effect.started_at_tick)
            .map_err(|_| {
                GeneralFailure::new(
                    GeneralFailureCode::InvalidEffect,
                    "effect duration is outside the supported range",
                )
            })?;
        applied.stacks = effect.stacks;
        results.push(applied);
        if let Some(active) = state
            .active_effects
            .get_mut(&target)
            .and_then(|effects| effects.get_mut(&key))
        {
            active.next_tick = active
                .next_tick
                .checked_add(u64::from(active.period_ticks))
                .ok_or_else(|| {
                    GeneralFailure::new(
                        GeneralFailureCode::TickOverflow,
                        "effect schedule overflow",
                    )
                })?;
        }
    }
    state
        .active_effects
        .retain(|_, effects| !effects.is_empty());
    Ok(results)
}

fn scale_operation(
    operation: &EffectOperation,
    stacks: i64,
) -> Result<EffectOperation, GeneralFailure> {
    let overflow = || {
        GeneralFailure::new(
            GeneralFailureCode::InvalidEffect,
            "stack-scaled effect amount exceeds the supported numeric range",
        )
    };
    Ok(match operation {
        EffectOperation::Damage { resource, amount } => EffectOperation::Damage {
            resource: resource.clone(),
            amount: u32::try_from(u64::from(*amount) * stacks as u64).map_err(|_| overflow())?,
        },
        EffectOperation::Heal { resource, amount } => EffectOperation::Heal {
            resource: resource.clone(),
            amount: u32::try_from(u64::from(*amount) * stacks as u64).map_err(|_| overflow())?,
        },
        EffectOperation::ResourceDelta { resource, delta } => EffectOperation::ResourceDelta {
            resource: resource.clone(),
            delta: i32::try_from(i64::from(*delta) * stacks).map_err(|_| overflow())?,
        },
        EffectOperation::SetTag { .. } => {
            return Err(GeneralFailure::new(
                GeneralFailureCode::InvalidEffect,
                "tag effects cannot be periodic or stack-scaled",
            ));
        }
    })
}

fn apply_npc_policy(
    snapshot: &GeneralGameSpec,
    state: &mut GeneralRuntimeState,
    npc: &EntityId,
    tick: u64,
) -> Result<NpcAction, GeneralFailure> {
    if !entity_alive(state, npc) {
        return Ok(NpcAction::Defeated { npc: npc.clone() });
    }
    let policy = match &snapshot.entities[npc].control {
        EntityControl::Player => return Ok(NpcAction::NoAction { npc: npc.clone() }),
        EntityControl::Npc { policy } => policy,
    };
    let NpcPolicy::UseAbilities { priority } = policy else {
        return Ok(NpcAction::NoAction { npc: npc.clone() });
    };
    for ability_id in priority {
        let ability = &snapshot.abilities[ability_id];
        let candidates = targeting_candidates(snapshot, state, npc, &ability.targeting);
        for target in candidates {
            if !ability_requirements_available(snapshot, state, npc, &target, ability_id, tick) {
                continue;
            }
            let transition = activate_ability(snapshot, state, npc, ability_id, &target, tick)?;
            if let GeneralTransition::AbilityActivated { effects, .. } = transition {
                return Ok(NpcAction::UsedAbility {
                    npc: npc.clone(),
                    ability: ability_id.clone(),
                    target,
                    effects,
                });
            }
        }
    }
    Ok(NpcAction::NoAction { npc: npc.clone() })
}

fn ability_requirements_available(
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
    actor: &EntityId,
    target: &EntityId,
    ability_id: &AbilityId,
    tick: u64,
) -> bool {
    let ability = &snapshot.abilities[ability_id];
    if ability
        .requirements
        .iter()
        .any(|requirement| !requirement_met(requirement, snapshot, state, actor, target))
    {
        return false;
    }
    let ready_at = state
        .cooldown_ready_at
        .get(actor)
        .and_then(|entries| entries.get(ability_id))
        .copied()
        .unwrap_or(0);
    if tick < ready_at {
        return false;
    }
    cost_shortfall(state, actor, target, &ability.costs).is_none()
}

fn cost_shortfall(
    state: &GeneralRuntimeState,
    actor: &EntityId,
    target: &EntityId,
    costs: &[ResourceCost],
) -> Option<(EntityId, ResourceId, u64, u32)> {
    let mut totals = BTreeMap::<(EntityId, ResourceId), u64>::new();
    for cost in costs {
        let payer = match cost.payer {
            CostPayer::Actor => actor,
            CostPayer::SelectedTarget => target,
        };
        *totals
            .entry((payer.clone(), cost.resource.clone()))
            .or_default() += u64::from(cost.amount);
    }
    totals
        .into_iter()
        .find_map(|((payer, resource), required)| {
            let available = state
                .entities
                .get(&payer)
                .and_then(|entity| entity.resources.get(&resource))
                .map_or(0, |resource| resource.current);
            (u64::from(available) < required).then_some((payer, resource, required, available))
        })
}

fn targeting_candidates(
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
    actor: &EntityId,
    rule: &TargetingRule,
) -> Vec<EntityId> {
    let Some(actor_state) = state.entities.get(actor) else {
        return Vec::new();
    };
    match rule {
        TargetingRule::SelfOnly => vec![actor.clone()],
        TargetingRule::Entity {
            required_tags,
            forbidden_tags,
            max_navigation_steps,
            allow_self,
        } => {
            let mut candidates: Vec<_> = state
                .entities
                .iter()
                .filter(|(candidate, entity)| {
                    (*allow_self || *candidate != actor)
                        && entity_alive(state, candidate)
                        && required_tags.iter().all(|tag| entity.tags.contains(tag))
                        && forbidden_tags.iter().all(|tag| !entity.tags.contains(tag))
                        && max_navigation_steps.is_none_or(|max_steps| {
                            shortest_path_steps(
                                &snapshot.navigation,
                                &actor_state.location,
                                &entity.location,
                            )
                            .is_some_and(|steps| steps <= usize::from(max_steps))
                        })
                })
                .map(|(candidate, _)| candidate.clone())
                .collect();
            candidates.sort_by_key(|candidate| {
                let location = &state.entities[candidate].location;
                (
                    shortest_path_steps(&snapshot.navigation, &actor_state.location, location)
                        .unwrap_or(usize::MAX),
                    candidate.clone(),
                )
            });
            candidates
        }
    }
}

fn target_is_valid(
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
    actor: &EntityId,
    target: &EntityId,
    rule: &TargetingRule,
) -> bool {
    targeting_candidates(snapshot, state, actor, rule).contains(target)
}

fn effect_recipients(
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
    actor: &EntityId,
    target: &EntityId,
    recipient: &EffectRecipient,
) -> Vec<EntityId> {
    match recipient {
        EffectRecipient::Actor => vec![actor.clone()],
        EffectRecipient::SelectedTarget => vec![target.clone()],
        EffectRecipient::AlliesInRange {
            max_navigation_steps,
        } => nearby_team_members(snapshot, state, actor, *max_navigation_steps, true),
        EffectRecipient::EnemiesInRange {
            max_navigation_steps,
        } => nearby_team_members(snapshot, state, actor, *max_navigation_steps, false),
    }
}

fn nearby_team_members(
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
    actor: &EntityId,
    max_steps: u16,
    allies: bool,
) -> Vec<EntityId> {
    let Some(actor_state) = state.entities.get(actor) else {
        return Vec::new();
    };
    state
        .entities
        .iter()
        .filter(|(candidate, entity)| {
            entity_alive(state, candidate)
                && same_team(&actor_state.control, &entity.control) == allies
                && shortest_path_steps(
                    &snapshot.navigation,
                    &actor_state.location,
                    &entity.location,
                )
                .is_some_and(|steps| steps <= usize::from(max_steps))
        })
        .map(|(candidate, _)| candidate.clone())
        .collect()
}

fn same_team(left: &EntityControl, right: &EntityControl) -> bool {
    matches!(left, EntityControl::Player) == matches!(right, EntityControl::Player)
}

fn requirement_met(
    requirement: &AbilityRequirement,
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
    actor: &EntityId,
    target: &EntityId,
) -> bool {
    let actor_state = &state.entities[actor];
    let target_state = &state.entities[target];
    match requirement {
        AbilityRequirement::ActorHasTags { tags } => {
            tags.iter().all(|tag| actor_state.tags.contains(tag))
        }
        AbilityRequirement::ActorLacksTags { tags } => {
            tags.iter().all(|tag| !actor_state.tags.contains(tag))
        }
        AbilityRequirement::TargetHasTags { tags } => {
            tags.iter().all(|tag| target_state.tags.contains(tag))
        }
        AbilityRequirement::TargetLacksTags { tags } => {
            tags.iter().all(|tag| !target_state.tags.contains(tag))
        }
        AbilityRequirement::ActorResourceAtLeast { resource, amount } => actor_state
            .resources
            .get(resource)
            .is_some_and(|value| value.current >= *amount),
        AbilityRequirement::TargetResourceAtLeast { resource, amount } => target_state
            .resources
            .get(resource)
            .is_some_and(|value| value.current >= *amount),
        AbilityRequirement::ObjectiveSecured { objective } => {
            state.objectives.get(objective) == Some(&ObjectiveStatus::Secured)
        }
        AbilityRequirement::ObjectiveUnsecured { objective } => {
            snapshot.objectives.contains_key(objective)
                && state.objectives.get(objective) != Some(&ObjectiveStatus::Secured)
        }
    }
}

fn objective_requirement_met(
    requirement: &ObjectiveRequirement,
    _snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
) -> bool {
    match requirement {
        ObjectiveRequirement::AllEntitiesDefeatedWithTags { tags } => state
            .entities
            .iter()
            .filter(|(_, entity)| tags.iter().all(|tag| entity.tags.contains(tag)))
            .all(|(id, _)| !entity_alive(state, id)),
        ObjectiveRequirement::EntityHasTag { entity, tag } => state
            .entities
            .get(entity)
            .is_some_and(|runtime| runtime.tags.contains(tag)),
        ObjectiveRequirement::ResourceAtLeast {
            entity,
            resource,
            amount,
        } => state
            .entities
            .get(entity)
            .and_then(|runtime| runtime.resources.get(resource))
            .is_some_and(|value| value.current >= *amount),
        ObjectiveRequirement::ObjectiveSecured { objective } => {
            state.objectives.get(objective) == Some(&ObjectiveStatus::Secured)
        }
    }
}

fn entity_alive(state: &GeneralRuntimeState, id: &EntityId) -> bool {
    state.entities.get(id).is_some_and(|entity| {
        entity
            .vital_resource
            .as_ref()
            .and_then(|resource| entity.resources.get(resource))
            .is_none_or(|resource| resource.current > 0)
    })
}

fn any_player_alive(snapshot: &GeneralGameSpec, state: &GeneralRuntimeState) -> bool {
    snapshot.entities.iter().any(|(id, entity)| {
        matches!(entity.control, EntityControl::Player) && entity_alive(state, id)
    })
}

fn all_required_objectives_secured(
    snapshot: &GeneralGameSpec,
    state: &GeneralRuntimeState,
) -> bool {
    snapshot.objectives.iter().all(|(id, objective)| {
        !objective.required_for_victory
            || state.objectives.get(id) == Some(&ObjectiveStatus::Secured)
    })
}

fn require_living_player(state: &GeneralRuntimeState, id: &EntityId) -> Result<(), GeneralFailure> {
    let Some(entity) = state.entities.get(id) else {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::UnknownEntity,
            id.as_str(),
            "entity is undeclared",
        ));
    };
    if !matches!(entity.control, EntityControl::Player) {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::EntityNotPlayable,
            id.as_str(),
            "only player-controlled entities receive direct input",
        ));
    }
    if !entity_alive(state, id) {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::EntityDefeated,
            id.as_str(),
            "defeated player entity cannot act",
        ));
    }
    Ok(())
}

fn validate_targeting(rule: &TargetingRule) -> Result<(), GeneralFailure> {
    if let TargetingRule::Entity {
        required_tags,
        forbidden_tags,
        ..
    } = rule
    {
        check_limit(
            "targeting required tag",
            required_tags.len(),
            GENERAL_MAX_TAGS_PER_RULE,
        )?;
        check_limit(
            "targeting forbidden tag",
            forbidden_tags.len(),
            GENERAL_MAX_TAGS_PER_RULE,
        )?;
        if !required_tags.is_disjoint(forbidden_tags) {
            return Err(GeneralFailure::new(
                GeneralFailureCode::InvalidAbility,
                "targeting cannot require and forbid the same tag",
            ));
        }
        for tag in required_tags.iter().chain(forbidden_tags) {
            validate_identifier(tag.as_str(), "targeting tag")?;
        }
    }
    Ok(())
}

fn validate_requirement(
    requirement: &AbilityRequirement,
    snapshot: &GeneralGameSpec,
) -> Result<(), GeneralFailure> {
    let tags = match requirement {
        AbilityRequirement::ActorHasTags { tags }
        | AbilityRequirement::ActorLacksTags { tags }
        | AbilityRequirement::TargetHasTags { tags }
        | AbilityRequirement::TargetLacksTags { tags } => Some(tags),
        _ => None,
    };
    if let Some(tags) = tags {
        if tags.is_empty() {
            return Err(GeneralFailure::new(
                GeneralFailureCode::InvalidRequirement,
                "tag requirements cannot be empty",
            ));
        }
        check_limit("requirement tag", tags.len(), GENERAL_MAX_TAGS_PER_RULE)?;
        for tag in tags {
            validate_identifier(tag.as_str(), "requirement tag")?;
        }
    }
    match requirement {
        AbilityRequirement::ActorResourceAtLeast { resource, .. }
        | AbilityRequirement::TargetResourceAtLeast { resource, .. } => {
            validate_identifier(resource.as_str(), "requirement resource")?;
            if !snapshot
                .entities
                .values()
                .any(|entity| entity.resources.contains_key(resource))
            {
                return Err(GeneralFailure::new(
                    GeneralFailureCode::InvalidRequirement,
                    format!(
                        "resource {:?} is not declared by any entity",
                        resource.as_str()
                    ),
                ));
            }
        }
        AbilityRequirement::ObjectiveSecured { objective }
        | AbilityRequirement::ObjectiveUnsecured { objective }
            if !snapshot.objectives.contains_key(objective) =>
        {
            return Err(GeneralFailure::new(
                GeneralFailureCode::InvalidRequirement,
                format!("objective {:?} is undeclared", objective.as_str()),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn validate_ability_bindings(
    snapshot: &GeneralGameSpec,
    ability_id: &AbilityId,
    ability: &GeneralAbilitySpec,
) -> Result<(), GeneralFailure> {
    let actors: Vec<_> = snapshot
        .entities
        .iter()
        .filter_map(|(id, entity)| {
            let can_use = match &entity.control {
                EntityControl::Player => true,
                EntityControl::Npc { policy } => match policy {
                    NpcPolicy::Passive => false,
                    NpcPolicy::UseAbilities { priority } => priority.contains(ability_id),
                },
            };
            can_use.then_some(id.clone())
        })
        .collect();
    let targets_by_actor: BTreeMap<_, _> = actors
        .iter()
        .map(|actor| {
            (
                actor.clone(),
                potential_targets(snapshot, actor, &ability.targeting),
            )
        })
        .collect();

    for cost in &ability.costs {
        let ids: Vec<_> = match cost.payer {
            CostPayer::Actor => actors.clone(),
            CostPayer::SelectedTarget => targets_by_actor
                .values()
                .flat_map(|targets| targets.iter().cloned())
                .collect(),
        };
        require_resource_on_entities(snapshot, &ids, &cost.resource, ability_id, "cost payer")?;
    }
    for requirement in &ability.requirements {
        match requirement {
            AbilityRequirement::ActorResourceAtLeast { resource, .. } => {
                require_resource_on_entities(
                    snapshot,
                    &actors,
                    resource,
                    ability_id,
                    "requirement actor",
                )?;
            }
            AbilityRequirement::TargetResourceAtLeast { resource, .. } => {
                let targets: Vec<_> = targets_by_actor
                    .values()
                    .flat_map(|values| values.iter().cloned())
                    .collect();
                require_resource_on_entities(
                    snapshot,
                    &targets,
                    resource,
                    ability_id,
                    "requirement target",
                )?;
            }
            _ => {}
        }
    }

    for effect in &ability.effects {
        let Some(resource) = effect_resource(&effect.operation) else {
            continue;
        };
        let mut all_recipients = Vec::new();
        for actor in &actors {
            let recipients = match &effect.recipient {
                EffectRecipient::Actor => vec![actor.clone()],
                EffectRecipient::SelectedTarget => {
                    targets_by_actor.get(actor).cloned().unwrap_or_default()
                }
                EffectRecipient::AlliesInRange {
                    max_navigation_steps,
                } => potential_team_recipients(snapshot, actor, *max_navigation_steps, true),
                EffectRecipient::EnemiesInRange {
                    max_navigation_steps,
                } => potential_team_recipients(snapshot, actor, *max_navigation_steps, false),
            };
            all_recipients.extend(recipients);
        }
        require_resource_on_entities(
            snapshot,
            &all_recipients,
            resource,
            ability_id,
            "effect recipient",
        )?;
    }
    Ok(())
}

/// Uses a wider set than current tag matches because abilities may add or
/// remove tags before a later activation. Resource contracts must still be
/// valid for every entity that could become a legal target in the world.
fn potential_targets(
    snapshot: &GeneralGameSpec,
    actor: &EntityId,
    rule: &TargetingRule,
) -> Vec<EntityId> {
    match rule {
        TargetingRule::SelfOnly => vec![actor.clone()],
        TargetingRule::Entity {
            max_navigation_steps,
            allow_self,
            ..
        } => {
            let Some(actor_spec) = snapshot.entities.get(actor) else {
                return Vec::new();
            };
            snapshot
                .entities
                .iter()
                .filter_map(|(id, entity)| {
                    if !allow_self && id == actor {
                        return None;
                    }
                    let in_range = max_navigation_steps.is_none_or(|max_steps| {
                        shortest_path_steps(
                            &snapshot.navigation,
                            &actor_spec.location,
                            &entity.location,
                        )
                        .is_some_and(|steps| steps <= usize::from(max_steps))
                    });
                    in_range.then_some(id.clone())
                })
                .collect()
        }
    }
}

fn potential_team_recipients(
    snapshot: &GeneralGameSpec,
    actor: &EntityId,
    max_steps: u16,
    allies: bool,
) -> Vec<EntityId> {
    let Some(actor_spec) = snapshot.entities.get(actor) else {
        return Vec::new();
    };
    snapshot
        .entities
        .iter()
        .filter_map(|(id, entity)| {
            let same_team = same_team(&actor_spec.control, &entity.control);
            let in_range =
                shortest_path_steps(&snapshot.navigation, &actor_spec.location, &entity.location)
                    .is_some_and(|steps| steps <= usize::from(max_steps));
            ((same_team == allies) && in_range).then_some(id.clone())
        })
        .collect()
}

fn effect_resource(operation: &EffectOperation) -> Option<&ResourceId> {
    match operation {
        EffectOperation::Damage { resource, .. }
        | EffectOperation::Heal { resource, .. }
        | EffectOperation::ResourceDelta { resource, .. } => Some(resource),
        EffectOperation::SetTag { .. } => None,
    }
}

fn require_resource_on_entities(
    snapshot: &GeneralGameSpec,
    ids: &[EntityId],
    resource: &ResourceId,
    ability: &AbilityId,
    description: &str,
) -> Result<(), GeneralFailure> {
    if !ids.iter().any(|id| {
        snapshot
            .entities
            .get(id)
            .is_some_and(|entity| entity.resources.contains_key(resource))
    }) {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::InvalidResource,
            ability.as_str(),
            format!(
                "no potential {description} declares resource {:?}",
                resource.as_str()
            ),
        ));
    }
    Ok(())
}

fn validate_effect(effect: &EffectSpec) -> Result<(), GeneralFailure> {
    match (effect.duration_ticks, effect.period_ticks) {
        (0, None) => {}
        (0, Some(_)) | (_, None) | (_, Some(0)) => {
            return Err(GeneralFailure::new(
                GeneralFailureCode::InvalidEffect,
                "instant effects have no period; timed effects need a positive period",
            ));
        }
        (duration, Some(_)) if duration > GENERAL_MAX_DURATION_TICKS => {
            return Err(GeneralFailure::new(
                GeneralFailureCode::LimitExceeded,
                "timed effect duration exceeds the supported bound",
            ));
        }
        _ => {}
    }
    if effect
        .period_ticks
        .is_some_and(|period| period > effect.duration_ticks)
    {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidEffect,
            "timed effect period cannot exceed its duration because it would never apply",
        ));
    }
    match &effect.operation {
        EffectOperation::Damage { resource, amount }
        | EffectOperation::Heal { resource, amount } => {
            validate_identifier(resource.as_str(), "effect resource")?;
            if *amount == 0 {
                return Err(GeneralFailure::new(
                    GeneralFailureCode::InvalidEffect,
                    "damage and healing amounts must be positive",
                ));
            }
        }
        EffectOperation::ResourceDelta { resource, delta } => {
            validate_identifier(resource.as_str(), "effect resource")?;
            if *delta == 0 {
                return Err(GeneralFailure::new(
                    GeneralFailureCode::InvalidEffect,
                    "resource delta must be nonzero",
                ));
            }
        }
        EffectOperation::SetTag { tag, .. } => {
            validate_identifier(tag.as_str(), "effect tag")?;
            if effect.duration_ticks > 0 {
                return Err(GeneralFailure::new(
                    GeneralFailureCode::InvalidEffect,
                    "periodic tag changes have no reversible expiry semantics",
                ));
            }
        }
    }
    if effect.duration_ticks == 0 && !matches!(effect.stacking, StackingPolicy::Replace) {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidEffect,
            "instant effects must use the neutral replace stacking policy",
        ));
    }
    if let StackingPolicy::AddStacks { max_stacks } = effect.stacking
        && max_stacks == 0
    {
        return Err(GeneralFailure::new(
            GeneralFailureCode::InvalidEffect,
            "stack cap must be positive",
        ));
    }
    let multiplier = match effect.stacking {
        StackingPolicy::AddStacks { max_stacks } => i64::from(max_stacks),
        StackingPolicy::Replace | StackingPolicy::Refresh => 1,
    };
    match &effect.operation {
        EffectOperation::Damage { amount, .. } | EffectOperation::Heal { amount, .. } => {
            if u64::from(*amount) * multiplier as u64 > u64::from(u32::MAX) {
                return Err(GeneralFailure::new(
                    GeneralFailureCode::InvalidEffect,
                    "stack cap can overflow the scaled effect amount",
                ));
            }
        }
        EffectOperation::ResourceDelta { delta, .. } => {
            if i32::try_from(i64::from(*delta) * multiplier).is_err() {
                return Err(GeneralFailure::new(
                    GeneralFailureCode::InvalidEffect,
                    "stack cap can overflow the scaled resource delta",
                ));
            }
        }
        EffectOperation::SetTag { .. } => {}
    }
    Ok(())
}

fn validate_objective_requirement(
    requirement: &ObjectiveRequirement,
    owner: &ObjectiveId,
    snapshot: &GeneralGameSpec,
) -> Result<(), GeneralFailure> {
    match requirement {
        ObjectiveRequirement::AllEntitiesDefeatedWithTags { tags } => {
            if tags.is_empty() {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidObjective,
                    owner.as_str(),
                    "defeat condition needs at least one selector tag",
                ));
            }
            check_limit(
                "objective selector tag",
                tags.len(),
                GENERAL_MAX_TAGS_PER_RULE,
            )?;
            for tag in tags {
                validate_identifier(tag.as_str(), "objective tag")?;
            }
            if !snapshot
                .entities
                .values()
                .any(|entity| tags.iter().all(|tag| entity.tags.contains(tag)))
            {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidObjective,
                    owner.as_str(),
                    "defeat condition selects no entities in the initial world",
                ));
            }
        }
        ObjectiveRequirement::EntityHasTag { entity, tag } => {
            if !snapshot.entities.contains_key(entity) {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidObjective,
                    owner.as_str(),
                    format!(
                        "objective references undeclared entity {:?}",
                        entity.as_str()
                    ),
                ));
            }
            validate_identifier(tag.as_str(), "objective tag")?;
        }
        ObjectiveRequirement::ResourceAtLeast {
            entity, resource, ..
        } => {
            let exists = snapshot
                .entities
                .get(entity)
                .is_some_and(|spec| spec.resources.contains_key(resource));
            if !exists {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidObjective,
                    owner.as_str(),
                    "objective resource condition references an undeclared entity resource",
                ));
            }
        }
        ObjectiveRequirement::ObjectiveSecured { objective } => {
            if objective == owner || !snapshot.objectives.contains_key(objective) {
                return Err(GeneralFailure::subject(
                    GeneralFailureCode::InvalidObjective,
                    owner.as_str(),
                    "objective dependency must refer to a different declared objective",
                ));
            }
        }
    }
    Ok(())
}

fn validate_objective_acyclic(snapshot: &GeneralGameSpec) -> Result<(), GeneralFailure> {
    fn visit(
        id: &ObjectiveId,
        snapshot: &GeneralGameSpec,
        temporary: &mut BTreeSet<ObjectiveId>,
        permanent: &mut BTreeSet<ObjectiveId>,
    ) -> bool {
        if permanent.contains(id) {
            return true;
        }
        if !temporary.insert(id.clone()) {
            return false;
        }
        for dependency in snapshot.objectives[id]
            .prerequisites
            .iter()
            .filter_map(|condition| {
                if let ObjectiveRequirement::ObjectiveSecured { objective } = condition {
                    Some(objective)
                } else {
                    None
                }
            })
        {
            if !visit(dependency, snapshot, temporary, permanent) {
                return false;
            }
        }
        temporary.remove(id);
        permanent.insert(id.clone());
        true
    }
    let mut temporary = BTreeSet::new();
    let mut permanent = BTreeSet::new();
    for id in snapshot.objectives.keys() {
        if !visit(id, snapshot, &mut temporary, &mut permanent) {
            return Err(GeneralFailure::subject(
                GeneralFailureCode::InvalidObjective,
                id.as_str(),
                "objective prerequisites contain a cycle",
            ));
        }
    }
    Ok(())
}

fn validate_identifier(value: &str, subject: &str) -> Result<(), GeneralFailure> {
    let mut chars = value.chars();
    let starts_with_letter = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic());
    let remaining_valid = chars
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'));
    if value.len() > MAX_IDENTIFIER_BYTES || !starts_with_letter || !remaining_valid {
        return Err(GeneralFailure::subject(
            GeneralFailureCode::InvalidIdentifier,
            subject,
            format!("identifier {value:?} is invalid or exceeds {MAX_IDENTIFIER_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn check_limit(kind: &str, found: usize, maximum: usize) -> Result<(), GeneralFailure> {
    if found > maximum {
        return Err(GeneralFailure::new(
            GeneralFailureCode::LimitExceeded,
            format!("{kind} count {found} exceeds maximum {maximum}"),
        ));
    }
    Ok(())
}

fn shortest_path_steps(
    graph: &BTreeMap<LocationId, BTreeSet<LocationId>>,
    start: &LocationId,
    goal: &LocationId,
) -> Option<usize> {
    if start == goal {
        return Some(0);
    }
    let mut visited = BTreeSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start.clone(), 0usize)]);
    while let Some((location, distance)) = queue.pop_front() {
        for neighbor in graph.get(&location)? {
            if neighbor == goal {
                return Some(distance + 1);
            }
            if visited.insert(neighbor.clone()) {
                queue.push_back((neighbor.clone(), distance + 1));
            }
        }
    }
    None
}

fn sha256_json<T: Serialize>(value: &T) -> Result<String, GeneralFailure> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        GeneralFailure::new(
            GeneralFailureCode::MalformedInput,
            format!("canonical JSON serialization failed: {error}"),
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
