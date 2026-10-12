## Purpose

Let users find contacts, track calls, and maintain favorites locally without external services or sharing their address book.

## ADDED Requirements

### Requirement: Local contacts and favorites
The application SHALL create, edit, remove, and search local contacts with names/multiple numbers and persist favorites. Selecting a contact SHALL populate a proposed call without unexpected automatic dialing.

#### Scenario: CH-01 contact and favorite
- **WHEN** a two-number contact is saved as a favorite and the application restarts
- **THEN** the contact/favorite reappear and the user can choose a number before confirming the call

### Requirement: Validated atomic CSV
The application SHALL import UTF-8 CSV with name and number columns, offer preview/duplicate counts, and reject invalid content without partial import. It SHALL enforce 10 MiB and 100,000-record limits per file.

#### Scenario: CH-02 invalid and duplicate CSV
- **WHEN** CSV contains repeated records, invalid numbers, or exceeds limits
- **THEN** preview reports the problems without partial writes, and valid files are applied only after confirmation

### Requirement: Coherent history
The application SHALL record call direction, destination, start, answer, end, outcome, and duration, persist missed calls, and allow filtering/confirmed redial. A crash-recovered session SHALL indicate interruption, never fabricated answering.

#### Scenario: CH-03 outcomes and recovery
- **WHEN** answered, missed, canceled calls and abrupt process interruption occur
- **THEN** each session appears exactly once with the correct outcome and duration includes only actually established time

### Requirement: Search and privacy
The application SHALL search names/numbers locally across contacts, favorites, and history without sending queries to external services. It SHALL allow history deletion and configurable retention, defaulting to 90 days.

#### Scenario: CH-04 search and retention
- **WHEN** the user searches a contact and confirms history deletion or retention expires
- **THEN** search generates no external traffic and removed entries disappear, without promising forensic erasure of backups
