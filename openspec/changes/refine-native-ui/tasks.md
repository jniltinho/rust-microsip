## 1. Approval and assets

- [ ] 1.1 Produce reference mockups from the design tokens (Phone idle, in call, incoming call, Calls, Contacts, Settings; light and dark) and get the owner's approval; verify the approved mockups and decisions are recorded. Concept images generated with Codex are in [docs/prints/proposal](../../../docs/prints/proposal/README.md), awaiting approval.
- [ ] 1.2 Pin the Material Symbols Sharp source by URL and SHA-256, add `packaging/fonts/subset-icons.sh` producing outline and filled subset fonts, and review the Apache-2.0 licence; verify byte-identical regeneration in CI and the licence in the app, AppImage and notices.

## 2. Tokens and styling

- [ ] 2.1 Replace the `tokens` module with the full role/spacing/type/size set and apply zero corner radius and flat states to `egui::Visuals` and `Style`; verify a contrast test for every role pair in both themes (UIR-01, UIR-02).
- [ ] 2.2 Load the icon fonts as a font family with a typed glyph table; verify every code point exists in the subset fonts and no runtime download occurs (UIR-03).

## 3. Screens

- [ ] 3.1 Restyle the tab strip, number field, keypad grid, edit row and call row; verify reducer tests unchanged and AT-SPI names intact (UIR-04).
- [ ] 3.2 Add the call panel with state strip, timer and security line, and icon toggles with on states exposed to AT-SPI; verify SRTP/RTP indication during lab calls (UIR-05).
- [ ] 3.3 Restyle Calls and Contacts lists, filters, empty states, incoming-call banner, Settings sections, contact editor, Import and About; verify keyboard selection and the Tab cycle.
- [ ] 3.4 Add density and scale preferences; verify 1024x768 at 100/150/200% with every action reachable (UIR-06).

## 5. MicroSIP parity (owner request, 2026-10-14)

- [ ] 5.1 Capture the real MicroSIP on Windows (and the Wine reference where Windows is unavailable) for every screen: Phone idle, Phone with a registered account (status icon Online), Logs/Calls with entries, Contacts, Settings/Account dialogs and the tray menu; verify captures with provenance (version, SHA-256, conditions) and no personal data.
- [ ] 5.2 Make the Calls tab match MicroSIP's Logs: one row per call with direction/outcome icon, name or number, date/time and duration columns, double-click to call back, context menu (call, copy, add to contacts, delete), search; verify against the 5.1 captures and the reducer tests.
- [ ] 5.3 Reorganize Settings into MicroSIP-like sections (Account, Audio devices with test and levels, Ringing/Notifications, Links/Integration, Agent access, About) with aligned labels, grouped panels and clear save/apply behaviour, AAA+ polish in both themes; verify keyboard navigation, AT-SPI names and contrast.
- [ ] 5.4 Add the status bar with a registration status icon (Online, Offline, Registering, Error, DND) and text as in MicroSIP; verify every state in the lab (registered, wrong password, network loss, DND).
- [ ] 5.5 Produce the side-by-side comparison of every 5.1 screen against rust-microsip in both themes, with a list of remaining differences; verify owner review.

## 4. Acceptance

- [ ] 4.1 Add per-screen, per-theme, per-scale captures with baseline comparison to the lab suite; verify a deliberate token change fails the comparison (UIR-06).
- [ ] 4.2 Produce the side-by-side matrix against the MicroSIP reference with listed differences; verify owner review (UIR-04).
- [ ] 4.3 Run an independent read-only Claude CLI review and the full lab suite; verify a receipt bound to the commit and capture hashes.
