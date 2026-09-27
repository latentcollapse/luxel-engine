# Gate Run vertical-slice brief

Build a compact, readable Codeweald arena in which a player character starts
at the west gate, traverses a connected terrain route, uses one deterministic
ability against a training warden, and claims the east objective. The slice
must be playable in the target runtime without editor-only setup.

Constraints:

- Unity is the target runtime for this MVP; Bevy remains an inspection oracle.
- The world uses right-handed XZ with Y up, a 64 m by 64 m playable extent,
  explicit collision and navigation artifacts, and one reachable objective.
- The character package must carry a verified source identity, a usable rig,
  idle and locomotion animation metadata, a capsule collision intent, and one
  gameplay socket.
- The gameplay loop is single-process and deterministic: two entities, one
  ability with a cost and cooldown, a warden objective, and explicit win/loss.
- Any missing, stale, or contradictory artifact fails closed. No primitive or
  silent fallback may replace a missing source asset.
