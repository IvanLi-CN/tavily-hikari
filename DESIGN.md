# Design

## Theme

Light tropical clay is the primary theme. The physical scene is an operator checking a self-hosted proxy on a bright laptop during daily maintenance: the UI should feel soft, safe, and inviting while keeping tables and logs efficient.

## Color

Use a full palette with restrained application in dense product surfaces.

- Canvas: pale lavender clay, `#F4F1FA`.
- Foreground: soft charcoal, `#332F3A`.
- Muted foreground: dark lavender gray, `#635F69`.
- Primary: vivid violet, `#7C3AED`.
- Secondary: hot pink, `#DB2777`.
- Info: sky blue, `#0EA5E9`.
- Success: emerald, `#10B981`.
- Warning: amber, `#F59E0B`.

Use tinted OKLCH neutrals and avoid pure black or pure white in new CSS. Accent color should identify actions, selected state, health, and progress instead of filling inactive chrome.

## Typography

Use Nunito for headings, stat numbers, navigation emphasis, and major labels. Use DM Sans for body copy and controls. Keep monospace only for tokens, request paths, code snippets, and IDs.

## Elevation

Clay depth uses four-layer shadows:

- `clay-surface` for large panels and app shells.
- `clay-card` for repeated cards and modules.
- `clay-button` for high-convexity actions.
- `clay-pressed` for inputs, selected pills, and recessed data surfaces.

Hover lifts should be short and useful. Pressed states should use scale and inset shadow. Reduced motion disables decorative float and transform animation.

## Components

Buttons are rounded, at least 44px tall, and use tactile hover and active feedback. Inputs and textareas are recessed. Cards and panels are rounded but not nested unnecessarily. Tables keep compact scan rhythm with subtle row hover, clear headers, and accessible status badges.

## Layout

Public and user console pages may use larger soft compositions and ambient blobs. Admin pages use clay surfaces, clearer grouping, and restrained color. Mobile layouts keep the same material language with tighter padding, never sharp corners.
