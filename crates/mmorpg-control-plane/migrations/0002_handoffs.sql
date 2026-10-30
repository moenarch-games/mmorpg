-- Fenced cross-zone handoff records (#145, ADR 0007). One row per transfer;
-- `phase` is 0 prepared, 1 accepted (the decision point), 2 committed,
-- 3 aborted. An entity has at most one unsettled (prepared or accepted)
-- transfer. IDs are unsigned 64-bit values stored bit for bit.
CREATE TABLE mmorpg_coordination.handoffs (
  transfer_id bigint PRIMARY KEY,
  entity_id bigint NOT NULL,
  source_zone bigint NOT NULL CHECK (source_zone >= 0),
  source_host text NOT NULL,
  source_epoch bigint NOT NULL CHECK (source_epoch > 0),
  destination_zone bigint NOT NULL CHECK (destination_zone >= 0),
  destination_host text NOT NULL,
  destination_epoch bigint NOT NULL CHECK (destination_epoch > 0),
  phase smallint NOT NULL CHECK (phase BETWEEN 0 AND 3),
  CHECK (source_zone <> destination_zone)
);
CREATE UNIQUE INDEX handoffs_unsettled_entity
  ON mmorpg_coordination.handoffs (entity_id) WHERE phase IN (0, 1);
GRANT SELECT, INSERT, UPDATE ON mmorpg_coordination.handoffs TO mmorpg_zone_directory;
