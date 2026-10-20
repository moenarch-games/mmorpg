CREATE TABLE mmorpg_coordination.zones (
  zone_id bigint PRIMARY KEY CHECK (zone_id >= 0),
  epoch bigint NOT NULL CHECK (epoch > 0),
  host_id text NOT NULL,
  expires_at timestamptz NOT NULL,
  retired boolean NOT NULL DEFAULT false,
  renewal_token uuid NOT NULL DEFAULT gen_random_uuid()
);
DO $$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mmorpg_zone_directory') THEN
    CREATE ROLE mmorpg_zone_directory NOLOGIN;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mmorpg_zone_fence_reader') THEN
    CREATE ROLE mmorpg_zone_fence_reader NOLOGIN;
  END IF;
END $$;
GRANT USAGE ON SCHEMA mmorpg_coordination TO mmorpg_zone_directory, mmorpg_zone_fence_reader;
GRANT SELECT ON mmorpg_coordination.migrations TO mmorpg_zone_directory, mmorpg_zone_fence_reader;
GRANT SELECT, INSERT, UPDATE ON mmorpg_coordination.zones TO mmorpg_zone_directory;
GRANT SELECT (zone_id, epoch, host_id, expires_at, retired) ON mmorpg_coordination.zones TO mmorpg_zone_fence_reader;
-- SELECT FOR SHARE requires UPDATE privilege. Keep that privilege with the
-- directory owner and expose only this fixed, read-only locking operation.
-- https://www.postgresql.org/docs/17/ddl-priv.html
-- https://www.postgresql.org/docs/17/sql-createfunction.html
CREATE FUNCTION mmorpg_coordination.lock_current_writer(p_zone bigint,p_epoch bigint,p_host text)
RETURNS smallint
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path=pg_catalog,pg_temp
AS $$
DECLARE assignment mmorpg_coordination.zones%ROWTYPE;
BEGIN
  SELECT * INTO assignment FROM mmorpg_coordination.zones AS z WHERE z.zone_id=p_zone FOR SHARE;
  IF NOT FOUND OR assignment.epoch<>p_epoch OR assignment.host_id<>p_host OR assignment.retired THEN
    RETURN 0;
  END IF;
  -- Evaluate expiry AFTER obtaining the row lock, including any lock wait.
  IF assignment.expires_at<=pg_catalog.clock_timestamp() THEN RETURN 1; END IF;
  RETURN 2;
END
$$;
REVOKE ALL ON FUNCTION mmorpg_coordination.lock_current_writer(bigint,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION mmorpg_coordination.lock_current_writer(bigint,bigint,text) TO mmorpg_zone_fence_reader,mmorpg_zone_directory;
