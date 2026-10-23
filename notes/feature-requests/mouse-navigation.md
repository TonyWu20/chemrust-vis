# Feature Request: Mouse-Supported Camera Navigation

**Date:** 2026-07-04
**Phase:** Phase 2 (Mouse + Selection) — navigation subset
**Status:** Answers collected — ready for implementation planning

## Summary

Add mouse-driven camera navigation and atom selection to the TUI. Four interactions:

| Gesture | Behavior |
|---|---|
| **Left-drag** (plain) | Orbit: rotate the camera around the current camera target |
| **Alt + Left-drag** | Pan ("push the paper"): drag left, the scene moves right. Target moves opposite to drag direction in screen space |
| **Scroll wheel** | Zoom: scroll up = zoom in (decrease radius), scroll down = zoom out (increase radius) |
| **Click** (no drag) | Select the atom under the cursor (ray-cast pick) |

**Shift + Left-drag** is NOT camera navigation. It is the object-manipulation gesture
(move selected atoms on the projected plane). That is a separate Phase 2 feature
covering selection + transform. Tracked here for context but out of scope for
the navigation work.

## Current state

- `Camera` in `chemrust-vis-core/src/camera.rs` already provides `orbit(dθ, dφ)`, `zoom(dr)`, and `pan(dx, dy)` (camera-local right/up axes).
- `input.rs` in the bin crate handles keyboard only. Mouse capture is enabled (`EnableMouseCapture`) but mouse events are ignored.
- CONTEXT.md §2.2 defines Camera as "Manipulated via keyboard (wasd/qe/arrows) and mouse (drag to rotate, scroll to zoom)."

## Resolved questions

1. **Rotation pivot point** — The orbit pivot is the **current camera target** (option B). After panning with Alt+Drag, subsequent plain drags orbit around the new target, not the bounding-box center.
2. **Shift+Drag** — Corrected: Shift+Drag is for **moving selected objects**, not camera panning. It is a separate Phase 2 feature. Camera panning is Alt+Drag only.
3. **Mouse button** — **Left button** only for all three camera-navigation gestures. Right and middle buttons are unbound for now.
4. **Scroll-wheel zoom** — **In scope.** Scroll up decreases `radius`, scroll down increases it.
5. **Sensitivity / snipe mode** — See recommendation below.
6. **Keyboard coexistence** — Existing keyboard shortcuts (WASD orbit, Shift+WASD pan, +/- zoom, r reset) **stay as-is**. Mouse gestures do not replace or supersede them. Both input channels coexist.
7. **Atom click-select** — **In scope.** A short click (below the drag threshold) performs a ray-cast pick to select the atom under the cursor. A drag above the threshold is navigation, not selection.

## Sensitivity recommendation

### Base mapping: zoom-scaled

Use the camera radius as the scaling factor so the same screen-space drag
always produces the same visual movement regardless of zoom level.

```
dθ (rad)  = pixel_dx × S_rot / radius      // rotation
dy_world  = pixel_dy × radius / viewport_h  // pan (Alt+Drag)
dr        = pixel_scroll × S_zoom × radius  // scroll zoom
```

Where:
- `S_rot` is a tuning constant (start with `0.01` rad/pixel at radius=1, so
  at the default radius ≈ 30 Å, 1 pixel ≈ 0.17°).
- `viewport_h` is the current viewport height in Braille dot units.
- `S_zoom` is a tuning constant (start with `0.02`, so 1 scroll tick at
  radius=30 moves the radius by 0.6 Å).

### Snipe mode

Hold **Ctrl** while dragging to divide sensitivity by **10**.
- Normal drag: 1 pixel ≈ 0.17° at default zoom
- Ctrl+drag: 1 pixel ≈ 0.017° at default zoom

This gives a 10× finer control range without a separate UI toggle. The user
can hold Ctrl for fine adjustments and release for normal speed.

If 10× is not fine enough, a second tier (hold Ctrl+Shift, 100× reduction)
can be added later.

### Drag threshold

A click that moves fewer than **3 pixels** between `MouseButtonPressed` and
`MouseButtonReleased` is treated as a click (atom selection). Anything above
3 pixels is a drag (navigation). The threshold is in pixel units and is not
affected by the snipe modifier.

## Scope split with Shift+Drag (object move)

When Phase 2 selection is implemented, Shift+Drag will:
- Move the currently selected atoms on the camera's projected plane
- Use the same pixel-to-world mapping as Alt+Drag (`radius / viewport_h`)
- Not affect the camera target or angles

This is tracked as a separate feature: `mouse-object-move.md` (to be created
when the selection feature is scoped).

## Implementation notes

- Mouse events come from `crossterm::event::Event::Mouse(MouseEvent)`.
- `MouseEvent` has `kind` (Press/Release/Move), `button`, `modifiers`,
  `column`, `row`.
- Track `press_position` and `current_position` to compute drag delta.
- Use `ModifiersState` to detect Shift/Alt/Ctrl during drag.
- For scroll zoom, use `MouseEventKind::ScrollUp` / `ScrollDown` (crossterm
  encodes wheel as synthetic mouse-move events with these kind markers).
- For atom picking, project each atom to screen space, find the nearest one
  within a pick radius (e.g., 8 Braille dots).

## Trackball navigation (2026-10-14)

A trackball replaces the click-drag workflow. The user holds a keyboard
modifier and rolls the ball. No button press. crossterm
`EnableMouseCapture` already enables `?1003h` (all-mouse tracking) and
`?1006h` (SGR coordinates). The terminal therefore reports every pointer
move, with the keyboard modifier state, even with no button held.

### Mapping (accepted 2026-10-14)

| Hold while moving | Camera action |
|---|---|
| Shift | Orbit (1/10 in snipe mode) |
| Alt | Pan ("push the paper") |
| Ctrl | Zoom on the vertical axis (1/10 in snipe mode) |
| none | No camera action (reserved for hover) |

Two modifiers compose: their actions apply together.

- The left button is optional for all three actions.
- Plain left-drag still orbits (legacy mouse gesture).
- Ctrl-drag zooms on the vertical axis, matching Ctrl-move.
- Wheel zoom is unchanged.
- Snipe is no longer the held Ctrl modifier. Snipe mode is a key
  toggle: press `f` to switch 1/10 orbit and move-zoom sensitivity on
  and off. The status bar shows a `snipe` indicator while active.
- Move-zoom sensitivity: 10 pixels of travel equal one wheel tick
  (`S_ZOOM_MOVE` = 0.002).

### Sensitivity changes (2026-10-14)

The user reported the mouse was too sluggish. Changes:

- `S_ROT` 0.01 → 0.04 (4x orbit sensitivity).
- New `S_PAN` = 2.0 multiplies the base pan mapping (2x pan).
- Kitty renderer: mouse y-deltas now use 4 px/row (the pixel grid).
  Half-block uses 2 dots/row. Before the fix, vertical gestures ran at
  half strength in kitty mode.

### Decisions recorded

1. Mapping Shift/Alt/Ctrl accepted (user, 2026-10-14).
2. Snipe mode is a key toggle (`f`), not a held modifier (user,
   2026-10-14).
3. Raise mouse sensitivity: orbit 4x, pan 2x, plus the kitty y-unit
   fix (user request, 2026-10-14).

### Renderer fixes (2026-10-14)

Two latent renderer bugs surfaced while verifying the pan gesture.

1. **View matrix inversion.** `Camera::view_matrix()` inverted nalgebra
   `look_at_rh`. In nalgebra 0.33, `look_at_rh(eye, target, up)`
   already returns the world to camera view matrix. The inversion
   flipped the rotation and left a wrong translation. Every projection
   ran on a mirrored camera. `view_matrix()` now returns `look_at_rh`
   directly. `camera_pose()` returns the inverse (camera to world).
   That is what `pan` and `axis_directions` need. Consequence: the
   on-screen spin direction of orbit reverses from the pre-fix
   display, and depth sort follows true view depth.
2. **Data-centroid centering cancelled target pans.** `Viewport::render`
   re-centered on the data view-space bounding box every frame. A
   target pan shifts the data and the box midpoint equally. The screen
   output never changed. The view now anchors on the camera target
   (the view-space origin). Scale still fits the data extent.

Regression test: `camera_target_pan_shifts_scene_on_screen` in
`viewport.rs` locks both behaviors in.

### Pan direction verification (2026-10-14)

User report: horizontal trackball pan sometimes looked diagonal.
Verification result: the pan is screen-axis-aligned at every camera
angle. Two camera tests lock this in:

- `horizontal_pan_is_screen_axis_aligned_at_all_angles`: 54 theta/phi
  combinations, including the pole-blend up-vector band. A pure
  horizontal pan gives zero vertical screen displacement (< 1e-9).
- `pan_view_shift_matches_pan_vector_at_all_angles`: the scene
  view-space shift equals the pan vector exactly. No theta/phi
  coupling in screen motion.

Two perceptual effects explain the report:

1. Trackball drift. A "straight" horizontal roll always carries some
   vertical travel. The screen mapping is physically isotropic
   (kitty: 2 px wide x 4 px tall per cell, the 2:1 cell aspect), so
   the diagonal the user sees is the ball's real travel path. A left
   pan with slight downward drift moves the scene left-bottom.
2. World-space direction. The pan target moves along the camera
   right/up axes in world space. That world direction rotates with
   the camera angles, visible in the axis indicator. Screen motion
   itself does not couple to the angles.

Open question: an optional axis-lock filter (zero the weak axis when
the strong axis dominates by 3-4x) to suppress drift diagonals.
Pending user decision.

### Terminal requirement

The terminal must forward SGR mouse events with modifier bits. Kitty,
WezTerm, and Ghostty do. Under tmux, set `set -g mouse on` and use
tmux 3.1 or newer.
