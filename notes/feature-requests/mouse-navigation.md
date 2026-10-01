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
