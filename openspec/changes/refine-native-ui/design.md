## Context

The v0.1 desktop UI (egui/eframe 0.36 + wgpu) implements the MicroSIP flow with default egui styling. Its behaviour is driven by the reducer in `apps/rust-microsip/src/ui/state.rs`; drawing lives in `ui/mod.rs` with a small `tokens` module. Accessibility is verified over AT-SPI and the lab suite captures real windows. See [proposal.md](proposal.md) and the [spec](specs/native-ui-refinement/spec.md).

References: MicroSIP Lite 3.22.16 (layout and density, captured in `docs/prints/microsip-reference.png`), flat square-cornered desktop design (Windows Metro/Fluent "sharp", Swiss-style grids, monochrome dialers such as Nothing-inspired apps), and compact desktop softphones that keep the dial pad central (RingCentral's compact mode). The goal is a calm, precise, high-contrast tool, not a decorative skin.

## Goals / Non-Goals

**Goals:** a coherent square-cornered visual language; one token source for colors, spacing, type and states; minimalist icons with text labels kept; the MicroSIP element order unchanged; light and dark themes at WCAG AA; crisp rendering at 100–200%; visual regression snapshots.

**Non-Goals:** pixel parity with MicroSIP or Win32 widgets; client-side window decorations; a theming engine; new runtime dependencies; changes to telephony, storage or the reducer's behaviour.

## Decisions

### Shape: no rounded corners
All `egui::Visuals` corner radii are 0 (widgets in every state, windows, menus, popups, text edits, scroll bars, selection). Separation comes from 1 px hairlines and surface steps, not shadows; popups get a 1 px border and no shadow. Focus is a 2 px square accent outline inside the control's bounds, so it never clips at the window edge.

### Tokens
One `tokens` module owns every value; widgets never use literal colors or sizes.

| Role | Light | Dark |
| --- | --- | --- |
| window | `#F4F5F7` | `#111316` |
| surface (fields, keys) | `#FFFFFF` | `#1A1D21` |
| surface-alt (hover, rows) | `#ECEEF1` | `#23272D` |
| pressed | `#DDE1E6` | `#2D3239` |
| hairline | `#D4D8DE` | `#30353C` |
| text | `#16191D` | `#E9EBEE` |
| text-muted | `#59626D` | `#9BA4AE` |
| accent (selection, focus, active tab) | `#0B63CE` | `#5AA2FF` |
| call (green) | `#1D7A35` | `#3FB950` |
| end / error (red) | `#C3262B` | `#FF6B66` |
| warning | `#8A4B00` | `#FFB74D` |

Every text/background and icon/background pair is checked at ≥ 4.5:1 (3:1 for large digits and icons) by a unit test; values move if a pair fails, roles do not.

Spacing is a 4 px grid (4/8/12/16). Control heights: key 44, call row 40, toggle 32, list row 40 (two lines 52), status bar 28. Type scale with the reviewed Noto Sans: 11 (captions, key letters), 13 (body), 16 (section titles), 20 (number field), 24 (keypad digits); digits must not shift while the call timer runs (verify Noto Sans figures are tabular, otherwise lay the timer out per character).

### Icons
Material Symbols **Sharp** (Google, Apache-2.0): square terminals match the square UI and the set covers telephony. The source variable font is pinned by URL and SHA-256; `packaging/fonts/subset-icons.sh` (fontTools in the Docker toolchain stage) instances two static weights — outline (FILL 0, wght 400) and filled (FILL 1) for "on" states — and subsets them to the glyphs used. The two small TTFs are committed with the licence and reproduced byte-for-byte by the script in CI. egui loads them as an extra font family; icons are drawn as glyphs, so they scale with the UI and inherit token colors. No new crate.

Initial glyphs: dialpad, history, contacts, menu, call, call_end, backspace, add, close, mic, mic_off, volume_up, do_not_disturb_on, pause, play_arrow, phone_forwarded, call_received, call_made, call_missed, star, search, lock, lock_open, settings, info, check, warning, person.

Icons are decorative for assistive technology: every control keeps its text label or accessible name (`named()`), and no state is conveyed by color or icon alone.

### Layout (MicroSIP skeleton kept)
Default window 312×480 (MicroSIP Lite client area 284×426), minimum unchanged; the main panel still scrolls when scaled.

1. **Tab strip** — Phone / Calls / Contacts with icon + label, active tab marked by a 2 px accent underline and text color; Menu as an icon button at the right.
2. **Call panel** (only during a call, replacing the number field as today) — direction icon, remote name and number, monospaced-width timer at the right, a 3 px left strip in the state color (ringing amber, connected green, held muted), and a security line: lock + "Encrypted (SRTP)" or open lock + "Unencrypted (RTP)" in warning color.
3. **Number field** — 20 px digits, flat surface, hairline bottom border that turns accent on focus, clear button inside the field.
4. **Keypad** — 3×4 flat keys on surface color separated by 1 px gaps (a grid, not floating buttons); digit 24 px with letters 11 px beneath; hover surface-alt, pressed darker; DTMF keys flash briefly when sent.
5. **Edit row** — backspace icon, +, C as quiet text/icon keys.
6. **Call row** — Call: full-height accent-green block with phone icon and label; End: red block with call_end icon. Disabled states keep layout and use muted fills.
7. **Levels** — speaker and microphone rows with icons and thin flat sliders (2 px track, square thumb).
8. **Toggles** — DND, MUTE, HOLD, XFER as equal-width icon+label buttons; "on" shows the filled icon, accent text and a 2 px accent top bar.
9. **Status bar** — 8 px square presence indicator (green registered, amber registering, red failed, grey disabled), account, and transport/security summary ("UDP · RTP").

Calls and Contacts use the same grid: filter chips become square segmented buttons; rows are two-line (name/number, time or label) with direction/favorite icons, hover highlight and keyboard focus row; empty states use one muted line and an icon. Settings groups fields into Account, Audio, Appearance and Integration sections with section titles and hairlines; the incoming-call banner becomes a full-width block with caller, Answer (green) and Reject (red).

### Density and scale
Comfortable (default, sizes above) and Compact (key 36, rows 32) densities; the scale preference (100/125/150/200%) multiplies egui's pixels-per-point on top of the system factor. At 1024×768 and 200% every action remains reachable (existing ND-05 gate).

### Acceptance
1. Approve reference mockups of the main screens in light and dark (built from these tokens; generated concepts in `docs/prints/proposal/`) with the owner before coding.
2. Implement tokens and styles first, then screens; keep the reducer untouched (its tests must pass unchanged).
3. Lab suite: add per-screen captures in both themes and 100/150/200%, compare against approved baselines with a small per-pixel tolerance (llvmpipe is deterministic), and keep the AT-SPI sweep and Tab cycle gates.
4. Side-by-side matrix with the MicroSIP reference: same element order and grouping, differences listed.

## Risks / Trade-offs

- egui has no OpenType feature control or letter-spacing → rely on font metrics; verify tabular digits early.
- An icon font can render blank if a glyph is missing → a test asserts every used code point exists in the subset font.
- Square, flat surfaces can look unfinished if spacing is inconsistent → tokens are the only source, snapshot review per screen.
- Accent green/red carry meaning → always paired with icon shape and text.
- Icon licence (Apache-2.0) must reach every distribution → added to licences screen, AppImage notices and the release SBOM.
