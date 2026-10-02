# Campaign 1: Window and present boundary

## Result

The pinned Lava API can create and present a real window on this machine. The
Rust boundary now exposes an engine-neutral window request and a strictly typed
capability receipt. Its bounded Lava probe opens a native window, creates a
Vulkan surface and swapchain through `RenderWindow`, acquires a swapchain image,
records a small diagnostic triangle through `WindowTarget`, then calls
`present_frame!`. It reports `present_verified` only when Lava advances its
in-flight frame slot after the present call. It never uses an offscreen capture
as present evidence.

This is a one-frame capability integration. The production graphics supervisor
still owns the existing offscreen capture path; this probe does not move scene
packets into a persistent interactive window, add an event loop, or create a
certification gate.

## Pinned API audit

The audit used the `graphics_lab` Julia environment and the pinned Lava source
at revision `11c7e31bdf62408d22bf379e9e59510f69d2103e`.

- `Lava.RenderWindow(width, height; ctx, title, vsync)` initializes GLFW,
  creates a Vulkan surface from its native window, and builds the swapchain.
- `Lava.WindowTarget(win)` names the current swapchain image as a graphics
  target. `Lava.draw!` records a raster pass into Lava's active batch.
- `Lava.acquire_next_image!` acquires an image and waits on its frame fence.
- `Lava.present_frame!(queue, win)` ends and submits the batch, signals the
  image-indexed render-finished semaphore, then invokes Vulkan queue present.
- `Lava.close(win)` waits for device work and explicitly destroys image views,
  synchronization objects, swapchain, surface, and GLFW window. The isolated
  probe calls it in `finally`; a process timeout kills the child if native
  initialization or presentation stalls.
- Lava handles `OUT_OF_DATE` and `SUBOPTIMAL` by rebuilding the swapchain and
  returns without advancing its frame slot. On the pinned implementation, a
  successful present advances `current_frame`. The Rust probe requires that
  transition and refuses to infer success from the `nothing` return value.

The API groups native window, surface, and swapchain creation in one constructor.
If that call fails, the receipt identifies the combined stage and leaves those
individual capabilities unestablished instead of guessing which substep failed.

## Typed contract and failure behavior

`WindowTargetRequest` binds a stable target ID, requested extent, and vsync
choice. `WindowProbeReceipt` records one of:

- `present_verified`, with pinned backend identity, Julia/device identity,
  observed framebuffer extent, one successful present, and every capability
  marked available;
- `unavailable`, only for a known preflight limitation such as no configured
  Julia project/runtime or no Linux display environment;
- `failed`, for a reached operation that errors, emits malformed evidence, uses
  a stale backend/protocol identity, times out, or cannot prove a present.

Unavailable and failed are not successes. Rust rejects a pass-shaped status
without complete evidence, any zero-present claim, stale Lava revision, unknown
receipt fields, or capability states that contradict the reported stage.
This receipt is diagnostic capability evidence. Integrating it into
certification authority requires a separately scoped registry change.

## Machine exercise

The one-frame probe was exercised on Linux with Wayland/X11 environment support,
Vulkan 1.4, and an NVIDIA GeForce RTX 5060. Lava initialized the device, opened a
96×64 target, recorded a real `WindowTarget` triangle, and advanced its in-flight
frame slot from 1 to 2 after `present_frame!`. The Vulkan loader printed a
third-party LSFG implicit-layer warning and skipped that layer; it did not
prevent the present.

The integration test repeats the real probe when a Linux display is configured.
On a headless Linux runner it expects an explicit `unavailable` result and
asserts that presentation remains `not_exercised`.

## Remaining boundary

The proof covers Lava's surface, swapchain, image acquisition, draw submission,
and one present on this device. It does not prove resize longevity, sustained
frame pacing, input/event handling, scene-packet rendering into a window,
multi-window behavior, validation-layer cleanliness, or cross-platform
presentation. A future persistent window backend should own one `RenderWindow`
for its lifetime, poll native events, report resize/present outcomes through a
typed channel, and preserve explicit close ordering. It should not reuse the
probe process per frame.
