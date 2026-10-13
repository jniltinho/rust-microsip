## ADDED Requirements

### Requirement: Square-cornered visual language
The desktop UI SHALL draw every widget, panel, menu, popup, field and selection with zero corner radius, separate regions with 1 px hairlines or surface steps instead of shadows, and take every color, size and spacing value from one token source for the light and dark themes.

#### Scenario: UIR-01 no rounded corners
- **WHEN** each screen (Phone idle, Phone in call, incoming call, Calls, Contacts, contact editor, Import, Settings, About) is captured in light and dark themes
- **THEN** no control or container shows a rounded corner, and no widget uses a color or size outside the token set

#### Scenario: UIR-02 token contrast
- **WHEN** the token test evaluates every text, icon and focus color against the backgrounds it is drawn on
- **THEN** text pairs reach at least 4.5:1, large digits and icons at least 3:1, in both themes

### Requirement: Minimalist icons with labels
The UI SHALL use a single minimalist icon family with square terminals, shipped as a reviewed, subset font with its licence, and SHALL keep a text label or accessible name for every control; no state SHALL be conveyed by icon or color alone.

#### Scenario: UIR-03 icons present and accessible
- **WHEN** the application starts on a clean system and the AT-SPI sweep visits every screen
- **THEN** every icon glyph used by the UI exists in the bundled icon font, no network resource is fetched, every focusable control has a name, and toggled states are exposed as checked/pressed states

### Requirement: MicroSIP-compatible refined layout
The refined window SHALL keep MicroSIP's element order and grouping — tab strip with menu, number field or call panel, 3×4 keypad with letters, edit row, call/end row, levels, DND/MUTE/HOLD/XFER, status bar — while adding the call panel with security indication, icon toggles with visible on states, and two-line history/contact rows.

#### Scenario: UIR-04 comparable to MicroSIP
- **WHEN** the refined main window and the MicroSIP Lite reference are compared side by side with fictional data
- **THEN** a MicroSIP user finds each essential action in the same position and order, and every difference is listed in the visual matrix

#### Scenario: UIR-05 call security is always visible
- **WHEN** a call is established over SRTP or plain RTP
- **THEN** the call panel shows a lock and "Encrypted (SRTP)" or an open lock and "Unencrypted (RTP)" in warning color for the whole call

### Requirement: Visual regression and scaling
Refined screens SHALL have approved baseline captures per screen, theme and scale (100/150/200%), and the lab suite SHALL fail when a capture deviates beyond the agreed tolerance or when any existing accessibility gate fails.

#### Scenario: UIR-06 snapshots and scale
- **WHEN** the lab suite captures every screen in both themes at 100, 150 and 200% on a 1024x768 screen
- **THEN** captures match the approved baselines within tolerance, all actions stay reachable, and the AT-SPI and Tab-cycle gates pass
