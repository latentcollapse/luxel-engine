use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{FailureCode, GAMEPLAY_SNAPSHOT_SCHEMA, GameFailure};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub String);

impl EntityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for EntityId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocationId(pub String);

impl LocationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LocationId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub String);

impl AbilityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for AbilityId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameplayTag {
    Player,
    Enemy,
    Objective,
    Scout,
    Arcanist,
    Alive,
    Defeated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAttributes {
    pub health: u32,
    pub max_health: u32,
    pub energy: u32,
    pub max_energy: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    Playable,
    Npc { behavior: NpcBehavior },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NpcBehavior {
    GuardObjectiveCounterattack {
        attack_damage: u32,
        attack_range_steps: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySpec {
    pub control: Control,
    pub location: LocationId,
    pub attributes: EntityAttributes,
    pub tags: BTreeSet<GameplayTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationGraph {
    /// An undirected graph. Both directions of every edge must be present.
    pub adjacency: BTreeMap<LocationId, BTreeSet<LocationId>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetingRule {
    EnemyWithinNavigationSteps { max_steps: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GameplayEffect {
    Damage { amount: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilitySpec {
    pub id: AbilityId,
    pub energy_cost: u32,
    /// Number of complete action ticks after activation before reuse.
    pub cooldown_ticks: u32,
    pub targeting: TargetingRule,
    pub effect: GameplayEffect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectivePrerequisite {
    AllNpcsDefeated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectiveSpec {
    pub location: LocationId,
    pub prerequisite: ObjectivePrerequisite,
    pub tags: BTreeSet<GameplayTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSnapshot {
    pub schema_version: String,
    pub navigation: NavigationGraph,
    pub entities: BTreeMap<EntityId, EntitySpec>,
    pub ability: AbilitySpec,
    pub objective: ObjectiveSpec,
}

impl GameSnapshot {
    pub fn validate(&self) -> Result<(), GameFailure> {
        validate_snapshot(self)
    }

    pub fn playable_ids(&self) -> impl Iterator<Item = &EntityId> {
        self.entities
            .iter()
            .filter_map(|(id, entity)| matches!(entity.control, Control::Playable).then_some(id))
    }

    pub fn npc_ids(&self) -> impl Iterator<Item = &EntityId> {
        self.entities
            .iter()
            .filter_map(|(id, entity)| matches!(entity.control, Control::Npc { .. }).then_some(id))
    }
}

pub fn validate_snapshot(snapshot: &GameSnapshot) -> Result<(), GameFailure> {
    if snapshot.schema_version != GAMEPLAY_SNAPSHOT_SCHEMA {
        return Err(GameFailure::new(
            FailureCode::UnsupportedSchema,
            format!(
                "snapshot schema {:?} is not supported; expected {GAMEPLAY_SNAPSHOT_SCHEMA:?}",
                snapshot.schema_version
            ),
        ));
    }

    validate_identifier(snapshot.ability.id.as_str(), "ability id")?;

    let playable: Vec<_> = snapshot.playable_ids().collect();
    if playable.len() != 2 {
        return Err(GameFailure::new(
            FailureCode::PlayableEntityCount,
            format!(
                "the vertical slice requires exactly two playable entities; found {}",
                playable.len()
            ),
        ));
    }
    let npcs: Vec<_> = snapshot.npc_ids().collect();
    if npcs.len() != 1 {
        return Err(GameFailure::new(
            FailureCode::NpcCount,
            format!(
                "the vertical slice requires exactly one objective-guarding NPC; found {}",
                npcs.len()
            ),
        ));
    }

    validate_graph(&snapshot.navigation)?;
    for (id, entity) in &snapshot.entities {
        validate_identifier(id.as_str(), "entity id")?;
        validate_identifier(entity.location.as_str(), "entity location")?;
        if !snapshot.navigation.adjacency.contains_key(&entity.location) {
            return Err(GameFailure::subject(
                FailureCode::UnknownLocation,
                id.as_str(),
                format!(
                    "entity starts at unknown location {:?}",
                    entity.location.as_str()
                ),
            ));
        }
        validate_attributes(&entity.attributes, id)?;
        validate_entity_tags(entity, id)?;
        if let Control::Npc { behavior } = &entity.control {
            match behavior {
                NpcBehavior::GuardObjectiveCounterattack {
                    attack_damage,
                    attack_range_steps: _,
                } if *attack_damage == 0 => {
                    return Err(GameFailure::subject(
                        FailureCode::InvalidNpcBehavior,
                        id.as_str(),
                        "NPC counterattack damage must be positive",
                    ));
                }
                NpcBehavior::GuardObjectiveCounterattack { .. } => {}
            }
            if entity.attributes.energy != 0 || entity.attributes.max_energy != 0 {
                return Err(GameFailure::subject(
                    FailureCode::InvalidAttributes,
                    id.as_str(),
                    "NPC energy is not part of this contract and must be zero",
                ));
            }
        }
    }

    if snapshot.ability.energy_cost == 0 {
        return Err(GameFailure::subject(
            FailureCode::InvalidAbilityCost,
            snapshot.ability.id.as_str(),
            "the ability must have a positive energy cost",
        ));
    }
    if snapshot.ability.cooldown_ticks == 0 {
        return Err(GameFailure::subject(
            FailureCode::InvalidCooldown,
            snapshot.ability.id.as_str(),
            "the ability must declare at least one cooldown tick",
        ));
    }
    match snapshot.ability.effect {
        GameplayEffect::Damage { amount: 0 } => {
            return Err(GameFailure::subject(
                FailureCode::SilentAbility,
                snapshot.ability.id.as_str(),
                "damage effect amount is zero and cannot change runtime state",
            ));
        }
        GameplayEffect::Damage { .. } => {}
    }
    let npc_health = snapshot.entities[npcs[0]].attributes.health;
    let damage_per_cast = match snapshot.ability.effect {
        GameplayEffect::Damage { amount } => amount,
    };
    let required_casts = u64::from(npc_health).div_ceil(u64::from(damage_per_cast));
    let available_casts: u64 = playable
        .iter()
        .map(|id| {
            u64::from(snapshot.entities[*id].attributes.energy)
                / u64::from(snapshot.ability.energy_cost)
        })
        .sum();
    if available_casts < required_casts {
        return Err(GameFailure::subject(
            FailureCode::InsufficientEncounterResources,
            snapshot.ability.id.as_str(),
            format!(
                "the guard requires {required_casts} ability activations but playable entities can afford only {available_casts}"
            ),
        ));
    }
    if !snapshot.objective.tags.contains(&GameplayTag::Objective)
        || snapshot.objective.tags.contains(&GameplayTag::Alive)
        || snapshot.objective.tags.contains(&GameplayTag::Defeated)
    {
        return Err(GameFailure::new(
            FailureCode::InvalidObjective,
            "objective must include the objective tag and cannot use entity life-state tags",
        ));
    }
    validate_identifier(snapshot.objective.location.as_str(), "objective location")?;
    if !snapshot
        .navigation
        .adjacency
        .contains_key(&snapshot.objective.location)
    {
        return Err(GameFailure::new(
            FailureCode::UnknownLocation,
            format!(
                "objective references unknown location {:?}",
                snapshot.objective.location.as_str()
            ),
        ));
    }

    let npc_id = npcs[0];
    let npc_location = &snapshot.entities[npc_id].location;
    for player_id in &playable {
        let player_location = &snapshot.entities[*player_id].location;
        if shortest_path_steps(
            &snapshot.navigation,
            player_location,
            &snapshot.objective.location,
        )
        .is_none()
        {
            return Err(GameFailure::subject(
                FailureCode::UnreachableObjective,
                player_id.as_str(),
                format!(
                    "objective at {:?} is unreachable from playable spawn {:?}",
                    snapshot.objective.location.as_str(),
                    player_location.as_str()
                ),
            ));
        }
        if shortest_path_steps(&snapshot.navigation, player_location, npc_location).is_none() {
            return Err(GameFailure::subject(
                FailureCode::UnreachableNpc,
                player_id.as_str(),
                format!(
                    "NPC {:?} at {:?} is unreachable from playable spawn {:?}",
                    npc_id.as_str(),
                    npc_location.as_str(),
                    player_location.as_str()
                ),
            ));
        }
    }

    Ok(())
}

fn validate_identifier(value: &str, subject: &str) -> Result<(), GameFailure> {
    let mut chars = value.chars();
    let starts_with_letter = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic());
    let remaining_valid = chars
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'));
    if !starts_with_letter || !remaining_valid {
        return Err(GameFailure::subject(
            FailureCode::InvalidIdentifier,
            subject,
            format!(
                "identifier {value:?} must start with an ASCII letter and contain only letters, digits, '_', '-', or '.'"
            ),
        ));
    }
    Ok(())
}

fn validate_attributes(attributes: &EntityAttributes, id: &EntityId) -> Result<(), GameFailure> {
    if attributes.max_health == 0
        || attributes.health == 0
        || attributes.health > attributes.max_health
        || attributes.energy > attributes.max_energy
    {
        return Err(GameFailure::subject(
            FailureCode::InvalidAttributes,
            id.as_str(),
            "starting health must be in 1..=max_health and energy must be <= max_energy",
        ));
    }
    Ok(())
}

fn validate_entity_tags(entity: &EntitySpec, id: &EntityId) -> Result<(), GameFailure> {
    let has_player = entity.tags.contains(&GameplayTag::Player);
    let has_enemy = entity.tags.contains(&GameplayTag::Enemy);
    let has_runtime_state =
        entity.tags.contains(&GameplayTag::Alive) || entity.tags.contains(&GameplayTag::Defeated);
    let role_matches = match entity.control {
        Control::Playable => has_player && !has_enemy,
        Control::Npc { .. } => has_enemy && !has_player,
    };
    if !role_matches || has_runtime_state {
        return Err(GameFailure::subject(
            FailureCode::InvalidEntityTags,
            id.as_str(),
            "static tags must identify exactly one role (player or enemy) and cannot predeclare alive/defeated state",
        ));
    }
    Ok(())
}

fn validate_graph(graph: &NavigationGraph) -> Result<(), GameFailure> {
    if graph.adjacency.is_empty() {
        return Err(GameFailure::new(
            FailureCode::InvalidNavigationGraph,
            "navigation graph must contain at least one location",
        ));
    }
    for (location, neighbors) in &graph.adjacency {
        validate_identifier(location.as_str(), "navigation location")?;
        for neighbor in neighbors {
            if location == neighbor {
                return Err(GameFailure::subject(
                    FailureCode::InvalidNavigationGraph,
                    location.as_str(),
                    "navigation graph cannot contain a self-edge",
                ));
            }
            let Some(reverse_neighbors) = graph.adjacency.get(neighbor) else {
                return Err(GameFailure::subject(
                    FailureCode::UnknownLocation,
                    neighbor.as_str(),
                    format!(
                        "navigation edge from {:?} points to an undeclared location",
                        location.as_str()
                    ),
                ));
            };
            if !reverse_neighbors.contains(location) {
                return Err(GameFailure::subject(
                    FailureCode::InvalidNavigationGraph,
                    location.as_str(),
                    format!(
                        "navigation edge {:?} -> {:?} is missing its reverse edge",
                        location.as_str(),
                        neighbor.as_str()
                    ),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn shortest_path_steps(
    graph: &NavigationGraph,
    start: &LocationId,
    goal: &LocationId,
) -> Option<usize> {
    if start == goal {
        return Some(0);
    }
    let mut visited = BTreeSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start.clone(), 0usize)]);
    while let Some((location, distance)) = queue.pop_front() {
        for neighbor in graph.adjacency.get(&location)? {
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

pub(crate) fn is_playable(snapshot: &GameSnapshot, id: &EntityId) -> bool {
    snapshot
        .entities
        .get(id)
        .is_some_and(|entity| matches!(entity.control, Control::Playable))
}

pub(crate) fn npc_attack(snapshot: &GameSnapshot, id: &EntityId) -> Option<(u32, u16)> {
    let entity = snapshot.entities.get(id)?;
    match entity.control {
        Control::Npc {
            behavior:
                NpcBehavior::GuardObjectiveCounterattack {
                    attack_damage,
                    attack_range_steps,
                },
        } => Some((attack_damage, attack_range_steps)),
        Control::Playable => None,
    }
}
