# WGE Native Graphics Capability Matrix

Status: engine-neutral Lava checkpoint, 2026-09-28

This matrix distinguishes implemented evidence from architectural intent and
from capabilities that remain future work. “Green” means the capability is
covered by an authority-owned test or promoted receipt, not that it is feature
complete for a commercial game.

| Capability | Status | Evidence / boundary |
| --- | --- | --- |
| Rust-owned graphics packet | green | Closed `wge.graphics-scene-packet/v6`; exact-key parsing and Rust lowering |
| Rust receipt promotion | green | Native supervisor independently validates packet, backend identity, capture, measurements, and telemetry |
| Persistent Julia process | green | Typed worker protocol and persistent `LavaBackend` resource caches |
| Pinned Lava/Vulkan device | green | Lava commit and device identity are bound into the native receipt |
| Offscreen color/depth rendering | green | Real Lava framebuffer, depth attachment, readback, and deterministic PPM capture |
| sRGB texture handling | green | Typed RGBA8 upload, exact transfer conversion, and focused Julia tests |
| Terrain geometry | green | Rust-owned world heightfield lowered to deterministic GPU mesh |
| Authored mesh UV0 | green | Closed v6 packet channel; Rust cardinality/finite validation and typed Lava upload |
| Semantic objective landmark | green | Rust-lowered octagonal beacon with distinct albedo/emissive roles and landmark telemetry |
| Close-range perspective inspection | green | Typed objective-close packet, real Lava capture, Rust promotion, and deterministic digest check |
| Composed authored-world profile | green / quality-limited | Real Riverwatch instances plus named shrine through the native path; 40-instance 768x512 receipt is deterministic, while foliage/terrain fidelity remains a documented gap |
| Opaque mesh materials | partial | Bounded albedo, metallic, roughness, clearcoat, Cook–Torrance-style response, and role-sampled surface maps; richer graph semantics remain |
| Normal/roughness/occlusion/emissive material roles | green | Rust/Julia role-typed IDs, color-space validation, digest-bound procedural maps, and Lava descriptor bindings |
| Independent material profiles | green | Riverwatch lowers distinct terrain, stone, and foliage albedo identities; each mesh batch resolves its own typed descriptor set |
| Directional lighting | green | Typed light intent and Lava shader lowering |
| Analytic environment lighting | green | Typed sky-top, horizon, ground, fog, and exposure intent |
| HDR scene target and resolve | green | Linear HDR target, deterministic 2x resolve, tone mapping at final boundary |
| Graphics-pass GPU timestamps | green | Capability-gated Vulkan timestamp bracket around the native frame; Rust receipt preserves `null` on unsupported queues |
| Directional shadows | partial | Fixed 512x512 map and deterministic 4-tap PCF; no cascades, contact, or soft shadows |
| Prefiltered IBL | absent | No environment convolution or probe/residency contract yet |
| Temporal AA / history | absent | Resolve is spatial and deterministic, not temporal |
| Semantic instancing | green | Typed background/landmark/gameplay-critical importance; class-balanced telemetry |
| Semantic culling margins | green | Closed typed margins and Rust re-count of visible/culled classes |
| LOD / meshlets / streaming | absent | No residency or distance-quality contract yet |
| Deterministic foliage | partial | Thirty opaque crossed-mesh background instances are rendered and promoted; no production foliage system |
| Decals / particles / water | absent | Intentionally outside the current slice |
| Collision/navigation projection | green | Reference world artifact supplies authoritative fields; native packet projects route and gameplay markers |
| Gameplay-visible capture | green | Route, player, opponent, encounter, and objective evidence are measured and gated |
| Native gameplay loop | partial | Rust reference runtime completes traversal/gameplay; Lava currently renders a validated world snapshot rather than driving a real-time input loop |
| Bevy inspection oracle | green | Certified artifact revalidation, PNG provenance, and 22 viewer tests; Bevy is not semantic authority |
| Engine-neutral provenance | green | World, spatial, packet, backend, material, capture, and measurement identities are bound by receipts |
| Rigging/skinning/retargeting | deferred | Explicitly outside this checkpoint; supplied bad GLB remains a rejection control |
| Unity integration | deferred | Explicitly outside this checkpoint |

The matrix is intentionally conservative. “Partial” is not a pass-shaped
substitute for a missing capability; it identifies exactly which contract is
present and which production behavior is still absent.
