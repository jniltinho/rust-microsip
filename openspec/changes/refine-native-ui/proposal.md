## Why

The first desktop UI is functional and accessible but plain: default egui widgets, text-only toggles, rounded widget corners and no visual hierarchy beyond spacing. The owner asked for a refined, modern, premium-quality ("AAA+") interface: square corners, modern minimalist icons, and a layout that stays close to MicroSIP so existing MicroSIP users feel at home immediately.

## What Changes

- A square-cornered visual language: zero corner radius on every widget, panel, menu and popup; 1 px hairline separators; flat fills; a single accent color; no gradients or drop shadows except a 1 px popup border.
- Design tokens in one module: color roles for light and dark, a 4 px spacing grid, type scale with tabular digits, control heights, focus ring, motion durations.
- Minimalist icons from Material Symbols **Sharp** (Apache-2.0, square line ends that match the square UI), shipped as a subset icon font loaded by egui like the existing text font — no new crate.
- The MicroSIP layout kept in the same order: tab strip with Menu at the right, number field, 3×4 keypad with letters, Back / + / C row, wide Call and narrow End, speaker/microphone rows, DND / MUTE / HOLD / XFER, status bar. Refinements stay inside that skeleton:
  - icon + label tabs with an accent underline for the active tab;
  - keypad keys with the digit large and letters small beneath, flat hover and pressed states;
  - Call (accent green, phone icon) and End (red, hang-up icon) as the strongest elements;
  - call panel with remote name/number, monospaced timer, security badge (lock icon for SRTP, open-lock warning for RTP), and state color strip;
  - icon toggles for DND, mute, hold and transfer with a visible "on" state (accent bar plus filled icon) and text labels kept for accessibility;
  - Calls and Contacts rows with direction/favorite icons, two-line layout, hover row highlight, keyboard selection;
  - status bar with a square presence indicator, account and transport/security summary.
- Incoming-call banner, settings, contact editor, import and about screens restyled with the same tokens; the settings screen grouped into sections (Account, Audio, Appearance, Integration).
- An optional compact density and a 100/125/150/200% scale preference that does not break the 1024x768 rule.
- A reference mock (HTML, light and dark) approved by the owner before implementation, then pixel snapshots of the real window per screen and theme.

## Capabilities

### New Capabilities

- `native-ui-refinement`: square-cornered visual language, design tokens, icon font, refined MicroSIP-like layout and its visual acceptance.

### Modified Capabilities

None canonical yet. It refines the `native-desktop` requirements of `create-native-rust-linux-release` without weakening them: ND-01 layout comparability, ND-03/04 themes, ND-05 accessibility and scaling, and ND-06 failure states keep their criteria and gain visual-regression checks.

## Impact

- Code: `apps/rust-microsip/src/ui` (tokens, styles, widgets, screens); `ui/state.rs` behaviour is unchanged. No engine, storage or platform changes.
- Assets: one subset icon font (Material Symbols Sharp, Apache-2.0) plus its licence in the app's licences screen, the AppImage and third-party notices; reviewed like the Noto Sans font.
- Tests: token contrast (WCAG AA) for every color role, icon glyph presence, snapshot captures per screen/theme/scale in the lab suite, the AT-SPI sweep and Tab cycle unchanged.
- Release: proposed after v0.1.0's essential gates; it can ship in v0.1.x or v0.2 depending on the owner's decision. It does not change telephony behaviour or claim pixel parity with MicroSIP.
- Not in scope: custom window decorations, animations beyond short state transitions, theming plug-ins, macOS/Windows styling.
