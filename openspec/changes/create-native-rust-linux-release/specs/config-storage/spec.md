## Purpose

Provide simple local file/environment configuration and SQLite persistence with verifiable precedence, credential protection, and data preservation on failure.

## ADDED Requirements

### Requirement: Deterministic precedence
The application SHALL resolve each field in this order: CLI flags > process environment > explicit `.env` > explicit or XDG TOML > SQLite > defaults, distinguishing missing from invalid values. `.env` SHALL load only through an explicit path, never automatic current-directory discovery. It SHALL display setting provenance/effectiveness without exposing secrets.

#### Scenario: CFG-01 precedence and provenance
- **WHEN** the same field appears in every source
- **THEN** the highest-precedence source wins, the UI identifies it, and no temporary override is automatically persisted in SQLite

#### Scenario: CFG-02 invalid file
- **WHEN** an explicitly requested file is missing or contains an unknown type, enum, or field
- **THEN** validation fails before SIP registration and identifies the field/path without exposing secret content or silently falling back

### Requirement: Persistence and editing
The application SHALL save preferences and non-secret configuration in embedded SQLite under XDG directories and offer validated UI editing. Fields overridden by a higher source SHALL indicate the override and prevent false confirmation of an effective change.

#### Scenario: CFG-03 durable setting
- **WHEN** a non-overridden field is saved through the UI and the application restarts
- **THEN** the new value reappears without a database server or mandatory external configuration

#### Scenario: CFG-04 active external override
- **WHEN** the user tries to edit a UI field fixed by environment or TOML
- **THEN** the application identifies the source to change and does not claim an ineffective change was applied

### Requirement: Secrets outside the database
The application MUST NOT write plaintext SIP passwords to SQLite, logs, exports, or diagnostics. It SHALL store only a Secret Service reference when available; otherwise it SHALL accept an ephemeral secret from an explicit private file/environment or request a per-session password without preventing basic use. Private files/databases SHALL use 0600 permissions and directories 0700.

#### Scenario: CFG-05 credentials and missing keyring
- **WHEN** a password is supplied with or without Secret Service
- **THEN** registration can proceed, SQLite contains at most a secret reference, and exports contain no password, token, or media key

#### Scenario: CFG-06 insecure secret file
- **WHEN** a secret-containing `.env` or TOML file is readable by other users
- **THEN** the application refuses it and explains how to fix permissions without printing the secret

### Requirement: Migrations and failures preserve data
The application SHALL use versioned transactional migrations, consistent pre-migration backups, and reject future database versions. Full disk, locks, corruption, or migration errors MUST preserve the existing file and show recovery instructions, never silently recreate it.

#### Scenario: CFG-07 interrupted migration
- **WHEN** the process is interrupted during a migration
- **THEN** the next opening finds either an intact previous version or a complete new version with a recoverable backup and no partially applied state

#### Scenario: CFG-08 unavailable or future database
- **WHEN** the database is locked, corrupt, out of space, or has a future version
- **THEN** the application reports failure, does not delete data, and does not register an account before resolving mandatory storage

### Requirement: Deliberate import and export
The application SHALL export non-secret TOML and import configuration with explicit confirmation and a transaction, preserving external sources. Legacy GOnnect and MicroSIP INI settings MUST NOT be automatically read or converted.

#### Scenario: CFG-09 import without touching legacy data
- **WHEN** the user imports valid TOML and exports configuration
- **THEN** a sanitized preview allows confirmation of SQLite writes, and installed-application files remain intact
