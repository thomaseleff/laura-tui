//! Shared by the integration tests.

use std::time::Duration;

/// How long a client, a fake or a screen may take. Every step spawns processes (`laura`, `cmd.exe`
/// in a ConPTY, a fake's `ping` retry), and on a loaded Windows host many tests doing so at once
/// take up to ~9 s; a hang still fails, just later.
pub const DEADLINE: Duration = Duration::from_secs(30);
