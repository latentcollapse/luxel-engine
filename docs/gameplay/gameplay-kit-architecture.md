Luxel Gameplay Kit Architecture
Modular Gameplay Capability Pool, Genre Profiles, and Model-Assembled Custom Kits
Status
Design contract connected to the native reference-runtime slice; the broader
capability pool remains a staged design surface.

The current certified runtime emits and verifies `gameplay_kit.json`, a typed
`ResolvedKit` for `luxel.reference.vertical-slice` (the `action_rpg` profile plus
the `network.offline` capability). Rust gameplay authority re-resolves and
compares the kit before promoting the gameplay receipt. This proves the kit
architecture is load-bearing for the vertical slice without claiming that
every capability listed in this document has already been implemented.
This document defines how Luxel should represent reusable gameplay machinery for model-driven game development.
It is not a mandate to implement every listed genre immediately.
The primary architectural goal is:
Make common game-development systems reusable, composable, tunable, inspectable, and easy for a model to assemble without forcing the model to repeatedly reinvent established gameplay infrastructure.

1. Core thesis
Traditional game templates bundle large amounts of code around a genre:
FPS Template
RPG Template
Survival Template
That is useful for humans, but it creates artificial boundaries.
Luxel should instead maintain:
               Gameplay Capability Pool
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
       combat         inventory      movement
       abilities      equipment      traversal
       AI             quests         economy
       weapons        crafting       networking
       loot           dialogue       persistence
          │              │              │
          └──────────────┼──────────────┘
                         ▼
                 assembled game kit
Genres become semantic profiles over this shared capability pool.
For example:
Souls-like Kit
= action combat
+ stamina
+ committed animation
+ dodge/i-frame rules
+ checkpoint/death loop
+ equipment/progression
+ encounter reset
while:
Looter Shooter Kit
= shooter combat
+ weapons
+ abilities
+ RPG stats
+ loot generation
+ equipment
+ encounter instances
+ progression
+ co-op/persistence
The implementation should not care that one component is culturally associated with an RPG and another with a shooter.
2. The model's job
A model operating Luxel should not normally implement:
- health systems;
- inventories;
- cooldown systems;
- weapon fire modes;
- effect stacking;
- objective state;
- AI perception;
- loot tables;
- replication policies;
- checkpoint logic;
- crafting graphs;
from scratch.
Instead the model should reason at the design level:
“This is a third-person action RPG with deliberate combat, high player mobility, equipment-driven progression, Souls-like checkpoints, optional co-op, and a Diablo-style affix system.”

Luxel should then help translate that into an explicit capability graph.
game vision
    ↓
capability requirements
    ↓
kit/profile candidates
    ↓
custom capability graph
    ↓
dependency resolution
    ↓
tuning/specification
    ↓
generated game runtime
3. Luxel Gameplay Capability System
Luxel should eventually provide a common gameplay substrate analogous in spirit to systems such as gameplay ability frameworks, but designed specifically around model ergonomics and composability.
Working name:
Gameplay Capability System — GCS
GCS should own generic machinery such as:
entities
attributes
resources
tags
abilities/actions
requirements
costs
cooldowns
targeting
effects
status/state
events
objectives
AI policies
equipment
inventory
interaction
progression
The existing Luxel generalized gameplay contract is the embryonic form of this system.
GCS should remain:
- deterministic where appropriate;
- schema-driven;
- replayable;
- inspectable;
- model-facing;
- modular;
- testable independently of any particular genre.
4. Atomic capability pool
The long-term capability pool should be organized around reusable systems rather than genre labels.
Character/state
- attributes/stats
- health/resources
- status conditions
- tags
- factions
- resistances
- level/progression
- experience
- skill trees
- class/archetype state
Abilities/actions
- activation conditions
- resource costs
- cooldowns
- charges
- targeting
- channels
- casts
- interrupts
- instant effects
- duration effects
- stacking
- proc/trigger rules
- combos
- stance/state requirements
Combat
- melee
- ranged
- projectiles
- hitscan
- damage
- healing
- armor
- shields
- poise/stagger
- blocking
- parrying
- i-frames
- hit reactions
- critical hits
- damage types
- elemental/status buildup
- aggro/threat
Movement
- walking/running
- jumping
- crouching
- sprint
- dodge
- dash
- climbing
- mantle
- swimming
- flying
- grappling
- wall movement
- mounts
- vehicles
Weapons
- weapon archetypes
- fire modes
- reload
- ammunition
- recoil
- spread
- ADS
- damage falloff
- projectile ballistics
- weapon switching
- attachments
- durability
- movesets
Inventory/equipment
- inventory
- slots
- stacking
- weight
- encumbrance
- equipment
- consumables
- containers
- storage
- trading
Loot
- loot tables
- rarity
- affixes
- procedural item rolls
- drop conditions
- smart loot
- boss rewards
- world loot
- instanced/shared loot
Interaction
- interactables
- doors
- switches
- pickups
- conversations
- contextual actions
- world-state triggers
- usable devices
Objectives/quests
- objectives
- quest graphs
- prerequisites
- branching
- failure states
- rewards
- timed objectives
- world-state conditions
AI
- perception
- threat
- utility scoring
- patrol
- combat behavior
- cover
- group tactics
- navigation
- schedules
- faction relations
- boss phases
Survival
- hunger
- thirst
- temperature
- fatigue
- disease
- shelter
- resource ecology
- durability
- environmental hazards
Crafting/building
- recipes
- resource requirements
- crafting stations
- construction
- snapping
- structural integrity
- research/unlocks
- production chains
Economy
- currency
- vendors
- pricing
- scarcity
- trading
- auction systems
- sinks/sources
- regional economies
Narrative
- dialogue
- choices
- reputation
- faction state
- relationship state
- narrative flags
- cutscene triggers
- procedural barks
Persistence
- save/load
- character persistence
- world persistence
- checkpoints
- respawn
- world reset policy
- account state
Multiplayer
- authority model
- replication
- prediction
- reconciliation
- sessions
- matchmaking
- parties
- guilds
- shards/zones
- instancing
- anti-cheat boundaries
This pool should grow as real games demand capabilities.
5. Genre kits are profiles, not silos
A kit should primarily contain:
required capabilities
optional capabilities
forbidden/incompatible combinations
dependency rules
recommended tuning ranges
default tuning profile
validation rules
design doctrine
sample scenarios
tests
A genre kit should not duplicate implementations already present in GCS.
6. Action RPG meta-kit
Common capabilities:
third-person movement
melee/ranged combat
abilities
attributes
equipment
inventory
progression
enemy AI
boss phases
quests
checkpoints
Potential profiles:
Deliberate action RPG
- high attack commitment
- meaningful stamina economy
- strong stagger/poise
- limited cancellation
- deliberate dodge timing
- high enemy telegraph readability
Aggressive action RPG
- faster recovery
- high forward pressure
- health/resource recovery from aggression
- quicker movement
- larger cancel space
Technical action RPG
- stance systems
- combo routing
- larger moveset graph
- complex resource interactions
- more cancel/transition rules
These profiles can approximate broad design families without copying a specific copyrighted game.
7. Souls-like kit
Built from Action RPG machinery plus:
checkpoint/death loop
enemy reset policy
resource recovery after death
shortcut topology
committed attacks
stamina
dodge/i-frame rules
poise/stagger
boss phases
equipment/stat progression
The kit should expose tuning dimensions such as:
combat:
  commitment: 0.80
  aggression: 0.55
  mobility: 0.60
  stamina_pressure: 0.70
  stagger_importance: 0.75
  parry_dependency: 0.30
  cancel_freedom: 0.20
  tracking_strength: 0.50
These are semantic design controls.
They compile into lower-level values.
8. Shooter meta-kit
Core:
camera/aim
weapons
ballistics
damage
movement
AI perception
spawn/objective systems
Profiles:
- arena shooter
- movement shooter
- tactical shooter
- military shooter
- extraction shooter
- hero shooter
- looter shooter
A tactical profile may select:
low TTK
high weapon lethality
limited movement
strong recoil
cover importance
information scarcity
while a movement profile selects:
high velocity
air control
sliding/wall movement
large combat spaces
low movement commitment
9. Looter-shooter kit
Illustrates cross-genre composition particularly well.
ShooterKit
+
RPG progression
+
loot/rarity/affixes
+
ability system
+
equipment
+
encounter instances
+
quests
+
co-op
+
persistent character state
This should not require a bespoke "looter-shooter engine."
It is simply a proven capability composition with defaults and validation.
10. ARPG kit
Possible systems:
click/action movement
large enemy counts
abilities
cooldowns/resources
loot
affixes
buildcraft
procedural encounters
difficulty scaling
bosses
quests
persistent progression
Profiles:
- classic click-to-move
- direct-control action ARPG
- seasonal/live-service
- roguelite ARPG
11. Survival kit
resource collection
inventory
crafting
building
environment
temperature
food/water
durability
world persistence
AI ecology
day/night
weather
Optional composition:
SurvivalKit
+ ShooterKit
= survival FPS

SurvivalKit
+ RPGKit
= survival RPG

SurvivalKit
+ MMO systems
= persistent survival MMO
12. MMO meta-kit
The MMO profile should be treated less as a combat genre and more as a world/runtime topology.
Potential components:
persistent accounts
persistent characters
world persistence
replication
server authority
zones/shards
instances
grouping
guilds
chat
social graph
economy
trading
questing
world events
loot
progression
matchmaking
live operations
Then combat can be separately selected:
MMO
+ tab-target RPG combat

MMO
+ action RPG combat

MMO
+ FPS combat

MMO
+ survival systems
This is exactly why genre systems cannot be code silos.
13. Roguelike / roguelite kit
run lifecycle
procedural layout
randomized rewards
meta-progression
permadeath/reset policies
difficulty escalation
seed handling
encounter generation
build composition
Can combine with:
FPS
Action RPG
deckbuilder
platformer
survival
14. Immersive sim kit
Especially interesting for Luxel because it depends on systemic composition.
interaction
physics
AI perception
factions
environmental simulation
inventory
abilities
stealth
multiple-solution objectives
persistent world state
The kit's design doctrine should emphasize:
systems should compose rather than rely exclusively on authored solution paths.

15. Strategy kits
RTS
- unit selection
- formations
- production
- tech trees
- resources
- fog of war
- pathfinding
- combat groups
- AI macro/micro
- victory conditions
Grand strategy
- factions
- diplomacy
- economy
- territory
- logistics
- population
- technology
- warfare
- simulation clocks
City builder
- zoning
- construction
- production chains
- populations
- services
- traffic/logistics
- economy
- simulation metrics
16. Other useful kit profiles
Luxel may eventually ship semantic profiles for:
- platformer
- metroidvania
- stealth
- horror
- racing
- vehicle combat
- fighting games
- sports
- tower defense
- deckbuilder
- tactics RPG
- turn-based RPG
- sandbox
- colony simulation
- automation/factory game
- puzzle/adventure
- visual novel/narrative game
These are starting profiles, not separate architecture branches.
17. Meta-kits
Some abstractions cut across conventional genres strongly enough that they deserve first-class meta-kits.
Examples:
OnlineWorldKit
networking
persistence
accounts
sessions
world authority
replication
social
LiveServiceKit
seasons
events
entitlements
rotating content
analytics hooks
content versioning
migration
LootKit
rarity
affixes
rolls
drop tables
smart loot
economy hooks
BuildcraftKit
attributes
skills
abilities
equipment
synergies
respec
validation
EncounterKit
spawns
waves
encounter state
difficulty
objectives
boss phases
rewards
NarrativeKit
dialogue
quests
flags
reputation
relationships
branching
ProceduralRunKit
seed
run state
procedural generation
reward progression
reset/meta progression
CompetitiveKit
teams
rounds
scoring
match lifecycle
ranking
spectating
anti-cheat requirements
SimulationKit
agents
needs
schedules
economy
ecology
systemic interactions
time progression
18. Custom kit synthesis
The most important functionality is not choosing from existing presets.
It is allowing the model to build a custom kit.
Example request:
“Make a third-person sci-fi extraction game with Souls-like melee combat, Destiny-style loot/buildcraft, survival inventory pressure, and persistent social hubs.”

The model should be able to generate:
CustomKit
├── ThirdPersonMovement
├── ActionCombat
│   ├── stamina
│   ├── dodge
│   ├── stagger
│   └── committed attacks
├── ShooterWeapons
├── LootKit
├── BuildcraftKit
├── ExtractionLoop
├── SurvivalInventory
├── EncounterKit
├── PersistentCharacter
├── SocialHub
└── MultiplayerSession
Then Luxel validates the composition.
19. Capability dependency graph
Every capability should declare:
requires
provides
conflicts_with
optional_integrations
authoritative_state
runtime_cost_class
network_requirements
persistence_requirements
validation_suite
Example:
AbilityCooldowns

requires:
  - AbilitySystem
  - GameClock

provides:
  - cooldown_state

optional_integrations:
  - UI
  - AIPlanning
  - Networking

validation:
  - cooldown cannot go negative
  - replay deterministic
  - replicated cooldown agrees with authority
This lets kit assembly become dependency resolution rather than improvisation.
20. Kit compiler
Eventually the model-facing workflow should be:
Vision
  ↓
Kit planner
  ↓
Capability graph
  ↓
Dependency resolver
  ↓
Conflict checker
  ↓
Parameter/tuning synthesis
  ↓
Gameplay specification
  ↓
GCS runtime
The result is explicit and inspectable.
The model should be able to explain:
“I selected these 28 capabilities because of these requirements.”

21. Tuning layers
Avoid exposing thousands of raw parameters immediately.
Use hierarchical tuning.
design intent
    ↓
semantic profile
    ↓
subsystem profile
    ↓
runtime parameters
Example:
combat = deliberate_aggressive
may lower into:
attack_commitment
recovery windows
stamina costs
dodge duration
i-frame timing
hitstop
enemy tracking
poise values
Advanced users/models can override lower layers when required.
22. Validation as part of every kit
A kit should ship with tests.
A Souls-like kit might verify:
- attacks cannot cancel outside permitted windows;
- stamina costs apply consistently;
- invulnerability windows match the declared policy;
- checkpoints reset intended encounter state;
- recovery resource is placed/recoverable correctly;
- enemy tracking remains within declared bounds.
An FPS kit might verify:
- projectile/hitscan semantics;
- reload lifecycle;
- recoil limits;
- spawn safety;
- objective reachability.
This converts genre knowledge into executable doctrine.
23. Design doctrine
This is one of the most valuable pieces.
Each kit should include machine-readable or model-readable guidance about why systems exist.
Not:
dodge_iframes = 13
but:
High commitment combat requires attack readability and punish windows. Increasing tracking without adjusting dodge timing can invalidate the intended evasion model.

This lets Luna reason about design rather than blindly tuning numbers.
Markdown is perfectly acceptable for this layer.
24. Pre-assembled genre markdowns
The user-facing/model-facing system may expose profiles such as:
kits/
├── action_rpg.md
├── soulslike.md
├── fps.md
├── tactical_fps.md
├── looter_shooter.md
├── survival.md
├── arpg.md
├── mmo.md
├── roguelite.md
├── immersive_sim.md
├── rts.md
└── city_builder.md
These files describe:
- expected capability composition;
- design doctrine;
- common variants;
- recommended defaults;
- important tradeoffs;
- known incompatible assumptions.
They are recipes over the capability pool.
They are not the implementation.
25. Custom kit artifact
Every generated game should ultimately persist its resolved kit:
game_kit.json
or equivalent typed IR.
It should contain:
selected capabilities
capability versions
dependencies
profiles
parameter overrides
custom modules
tests
runtime requirements
network requirements
persistence requirements
This becomes part of project identity and provenance.
26. Custom game mechanics
Luxel must not force every mechanic into GCS.
A game may contain something genuinely unusual.
That should be represented as:
Known capability pool
        +
CustomCapability
Custom capability modules must still satisfy Luxel contracts:
- bounded authority;
- declared state;
- deterministic/replay semantics where required;
- dependencies;
- tests;
- evidence.
If a custom capability proves broadly useful across several games, it can later graduate into the common pool.
27. Kit evolution
The capability pool should be empirical.
Do not attempt to predict every future game mechanic.
Instead:
build game
↓
discover repeated subsystem
↓
generalize carefully
↓
prove across multiple games
↓
promote into shared pool
This mirrors the rest of Luxel's architecture.
28. Why this matters for model ergonomics
Without kits:
model
→ invent architecture
→ implement systems
→ integrate systems
→ tune systems
→ debug interactions
→ make game
With GCS + kits:
model
→ understand game vision
→ choose capability composition
→ tune
→ create genuinely unique pieces
→ make game
The model spends less capability on solved infrastructure.
It can spend more capability on:
- design
- atmosphere
- encounter construction
- world building
- mechanics
- balance
- narrative
- art direction
That is exactly Luxel's broader thesis.
29. Product structure
This also produces natural product/package layers:
Luxel Core
    ↓
Luxel GCS
    ↓
Capability Pool
    ↓
Genre / Meta-Kits
    ↓
Custom Game Kit
    ↓
Specific Game
Potential commercial distributions could eventually include:
Luxel Action RPG Kit
Luxel Shooter Kit
Luxel Survival Kit
Luxel MMO Systems Kit
Luxel Strategy Kit
But they should remain interoperable because they are all built from the same underlying capability system.
30. End-state workflow
The ideal interaction becomes:
“I want a four-player gothic science-fiction extraction RPG. Gunplay should be deliberate, melee should use high-commitment Souls-like principles, loot should support deep buildcraft, runs should last around 30 minutes, and players return to a persistent hub.”

Luxel/model:
interprets vision
    ↓
selects:
Shooter
+ ActionCombat
+ Extraction
+ Loot
+ Buildcraft
+ CoOp
+ Persistence
+ HubWorld
    ↓
resolves dependencies
    ↓
generates custom kit
    ↓
asks only unresolved design questions
    ↓
builds
That is much closer to:
describe the game you want

than:
describe every subsystem the game needs.

31. Architectural invariant
The most important rule:
Genre is metadata. Capability composition is architecture.

An MMO is not one codebase.
A Souls-like is not one codebase.
A looter-shooter is not one codebase.
They are recognizable regions in a much larger gameplay-system design space.
Luxel should expose those regions because they are useful to humans and models, but the engine underneath should see:
capabilities
dependencies
constraints
parameters
evidence
32. Immediate recommendation
Do not implement the full kit catalogue yet.
First build:
GCS core
+
capability registry
+
dependency graph
+
kit manifest schema
+
kit resolver
+
2-3 deliberately overlapping profiles
I’d use:
1. Action RPG / Souls-like
2. Shooter / Looter-shooter
3. Survival
because they force cross-kit composition almost immediately.
Then prove something like:
ShooterKit
+
LootKit
+
RPGBuildcraft
=
LooterShooter
without writing a new gameplay architecture.


The genre division is semantic. Everything lives in a shared capability pool. Genre kits are pre-assembled, documented compositions that a model may use directly, modify, or cannibalize to construct a custom kit for the exact game being built.
