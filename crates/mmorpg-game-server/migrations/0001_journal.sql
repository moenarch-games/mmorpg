CREATE TABLE mmorpg_persistence.streams (
  zone_id bigint PRIMARY KEY CHECK (zone_id >= 0),
  world_id uuid NOT NULL DEFAULT gen_random_uuid(),
  timeline bigint NOT NULL DEFAULT 1 CHECK (timeline > 0),
  writer_host text NOT NULL,
  writer_epoch bigint NOT NULL CHECK (writer_epoch > 0),
  retired boolean NOT NULL DEFAULT false,
  last_position bigint NOT NULL DEFAULT 0 CHECK (last_position >= 0),
  last_tick bigint NOT NULL DEFAULT 0 CHECK (last_tick >= 0),
  retained_bytes bigint NOT NULL DEFAULT 0 CHECK (retained_bytes >= 0),
  retained_batches bigint NOT NULL DEFAULT 0 CHECK (retained_batches >= 0),
  checkpoint_position bigint,
  checkpoint_tick bigint,
  checkpoint_ref bytea,
  CHECK ((checkpoint_ref IS NULL AND checkpoint_position IS NULL AND checkpoint_tick IS NULL)
      OR (checkpoint_ref IS NOT NULL AND checkpoint_position IS NOT NULL AND checkpoint_tick IS NOT NULL
          AND checkpoint_position >= 0 AND checkpoint_tick >= 0))
);
CREATE TABLE mmorpg_persistence.journal (
  zone_id bigint NOT NULL REFERENCES mmorpg_persistence.streams(zone_id),
  timeline bigint NOT NULL,
  position bigint NOT NULL CHECK (position > 0),
  tick bigint NOT NULL CHECK (tick >= 0),
  writer_epoch bigint NOT NULL CHECK (writer_epoch > 0),
  replay bytea NOT NULL,
  sessions bytea NOT NULL,
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  commit_digest bytea NOT NULL CHECK (octet_length(commit_digest) = 32),
  byte_length bigint NOT NULL CHECK (byte_length >= 0),
  PRIMARY KEY (zone_id, timeline, position)
);
-- An entitlement is unique independently of connection/session identity. The
-- producer includes the durable recipient in IDs for per-character rewards;
-- finite world-loot entitlements instead have a globally unique source ID.
CREATE TABLE mmorpg_persistence.grants (
  grant_id text PRIMARY KEY CHECK (octet_length(grant_id) BETWEEN 1 AND 256),
  character_id text NOT NULL REFERENCES mmorpg_characters.entries(character_id),
  zone_id bigint NOT NULL,
  world_id uuid NOT NULL,
  timeline bigint NOT NULL,
  journal_position bigint NOT NULL,
  record_revision bigint NOT NULL CHECK (record_revision > 0)
);
CREATE INDEX grants_by_character ON mmorpg_persistence.grants(character_id);
CREATE INDEX grants_by_world ON mmorpg_persistence.grants(world_id);
DO $$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mmorpg_zone_journal') THEN
    CREATE ROLE mmorpg_zone_journal NOLOGIN;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mmorpg_zone_coordinator') THEN
    CREATE ROLE mmorpg_zone_coordinator NOLOGIN;
  END IF;
END $$;
GRANT USAGE ON SCHEMA mmorpg_persistence TO mmorpg_zone_journal, mmorpg_zone_coordinator;
GRANT SELECT ON mmorpg_persistence.migrations TO mmorpg_zone_journal, mmorpg_zone_coordinator;
GRANT SELECT ON mmorpg_persistence.streams TO mmorpg_zone_journal;
GRANT UPDATE (last_position, last_tick, retained_bytes, retained_batches,
              checkpoint_position, checkpoint_tick, checkpoint_ref)
  ON mmorpg_persistence.streams TO mmorpg_zone_journal;
GRANT SELECT, INSERT, DELETE ON mmorpg_persistence.journal TO mmorpg_zone_journal;
GRANT SELECT, INSERT ON mmorpg_persistence.grants TO mmorpg_zone_journal;
GRANT SELECT, INSERT, UPDATE ON mmorpg_persistence.streams TO mmorpg_zone_coordinator;
GRANT mmorpg_characters_store TO mmorpg_zone_journal;

-- Cluster identity is read to reject an unrelated database as a replication witness.
GRANT EXECUTE ON FUNCTION pg_catalog.pg_control_system(), pg_catalog.pg_control_checkpoint() TO mmorpg_zone_journal;
