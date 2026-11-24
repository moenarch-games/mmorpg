-- Shared directories read without a lock and write on change (#1146): each row counts its
-- writes, so a host that already decoded the current revision reads only that number, and
-- names the format of its encoding, so a later layout decodes older ones instead of clearing
-- the row. A write keeps the encoding it replaced, which the repair step restores when the
-- current one cannot be decoded (MMORPG_PERSISTENCE_MODE=repair-directories).
ALTER TABLE mmorpg_coordination.party_directory
  ADD COLUMN revision bigint NOT NULL DEFAULT 0 CHECK (revision >= 0),
  ADD COLUMN format smallint NOT NULL DEFAULT 1,
  ADD COLUMN previous_revision bigint,
  ADD COLUMN previous_format smallint,
  ADD COLUMN previous_state bytea;
ALTER TABLE mmorpg_coordination.instance_directory
  ADD COLUMN revision bigint NOT NULL DEFAULT 0 CHECK (revision >= 0),
  ADD COLUMN format smallint NOT NULL DEFAULT 1,
  ADD COLUMN previous_revision bigint,
  ADD COLUMN previous_format smallint,
  ADD COLUMN previous_state bytea;

-- Settled handoff records are kept for a retention period, then deleted by the serving hosts
-- (#1146). Only settled (committed or aborted) rows carry a settlement time.
ALTER TABLE mmorpg_coordination.handoffs ADD COLUMN settled_at timestamptz;
UPDATE mmorpg_coordination.handoffs SET settled_at = now() WHERE phase IN (2, 3);
ALTER TABLE mmorpg_coordination.handoffs
  ADD CONSTRAINT handoffs_settled_at CHECK ((phase IN (2, 3)) = (settled_at IS NOT NULL));
CREATE INDEX handoffs_settled ON mmorpg_coordination.handoffs (settled_at)
  WHERE settled_at IS NOT NULL;
GRANT DELETE ON mmorpg_coordination.handoffs TO mmorpg_zone_directory;
