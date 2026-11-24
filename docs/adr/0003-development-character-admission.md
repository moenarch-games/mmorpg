# Development character admission and ownership

Status: implements owner Decision Atlas 3.1.5 and issue #30.

The development/LAN host may accept an unauthenticated durable CharacterId claim.
Storage acquisition happens outside simulation locks. The acquired record enters
core through a trusted, ordered game-server host input, so replay and recovery do
not reread external storage. Core validates and stages admission before replacing
the provisional session player. Gameplay commands are refused until binding.

`MMORPG_CHARACTER_DIR` enables this mode. One process exclusively locks that local
directory; adapters retain the lock for their entire lifetime. A persisted epoch
advances before startup and fences abandoned character leases from the stopped
previous process. This is a single development host, not production distributed
zone coordination. Multiple zones in that host share the character store.

Use the existing FileEntries/CharacterStore contracts. Periodic commits run on a
blocking worker, outside the tick. Logout parks the exact record through another
replayed host input, removes its live body, commits the record and releases its
lease. A failed periodic commit fails only its character (amended by #1145): the
pass saves every other character, retries it each pass and, after ten failed
passes in a row, logs its session out through the same parking input. Only a
newer epoch of one of the host's own zones holding a character stops serving, and
it stops through the graceful shutdown below, which commits and releases what the
host still holds. Abrupt connection loss retains the
last periodic commit after reconnect grace expires; stronger reward acknowledgement
and replicated crash journaling remain #40's responsibility.

Reuse game-server's whole-host graceful recovery, defaulting to
`<character-directory>/zone-recovery`. Before serving, inspect prepared runtimes
and reacquire every restored character lease. After graceful shutdown, prepare
that stopped recovery image without serving it, capture the final records, commit
and release them. Preparation leaves the bundle available for the next startup.
No character owns a private world and no persistence I/O runs inside gameplay.

Private canonical adapter snapshots use `MMOC` version 1 around the existing core
snapshot and stable player/character bindings. Parked players additionally carry
their existing versioned CharacterRecord bytes. Active durable facts come from the
core snapshot once. Client command, projection and character-record formats do not
change. Unsupported or corrupt recovery still fails closed.

Each client persists random online aliases for its local roster slots; deleted
slots with aliases stay retired. These aliases identify records, not authentication
or client-owned progression. Native `--character-id` deliberately overrides the
alias for trusted development use. Capability rejection retains anonymous-host
compatibility; an unsuccessful claim never becomes anonymous entry.

Amended by #1141: claims stay unauthenticated until account authentication
exists, so hosts listen on loopback unless `MMORPG_BIND_IP` names another
address, and the PostgreSQL persistence mode serves beyond loopback only with
`MMORPG_ALLOW_UNAUTHENTICATED_CLAIMS=1`. A session holds a zone slot before it
claims, so a zone that requires claims admits at most 32 unclaimed sessions and
removes one that has not claimed within 30 seconds, through game-server's
replayed admission expiry.
