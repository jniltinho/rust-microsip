## ADDED Requirements

### Requirement: Windows folders and precedence
On Windows, configuration SHALL default to `%APPDATA%\rust-microsip` and the database, backups and logs to `%LOCALAPPDATA%\rust-microsip`; explicit paths SHALL override them as on Linux. Precedence, provenance, migrations and import/export SHALL behave exactly as on Linux, and a configuration exported on one OS SHALL import on the other without secrets.

#### Scenario: WIN-09 first run
- **WHEN** a new Windows user starts rust-microsip and saves settings
- **THEN** `config.toml` is under `%APPDATA%\rust-microsip`, the database under `%LOCALAPPDATA%\rust-microsip`, and both directories carry the owner-only access-control list

#### Scenario: WIN-09 cross-OS import
- **WHEN** a TOML exported on Linux is imported on Windows with preview and apply
- **THEN** settings match, no password is imported, and the source file is untouched

#### Scenario: WIN-09 invalid folder variable
- **WHEN** `%LOCALAPPDATA%` is empty or not an absolute path
- **THEN** the application refuses to create data there and names the problem
