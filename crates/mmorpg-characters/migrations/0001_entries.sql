-- Character entries (TECH-002): one opaque, versioned entry per character,
-- in their own schema, apart from the control plane's zone directory.
CREATE TABLE mmorpg_characters.entries (
  character_id text PRIMARY KEY,
  entry bytea NOT NULL
);

-- Hosts connect as a login role granted this one; it can read and write
-- entries but not change the schema.
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mmorpg_characters_store') THEN
    CREATE ROLE mmorpg_characters_store NOLOGIN;
  END IF;
END
$$;
GRANT USAGE ON SCHEMA mmorpg_characters TO mmorpg_characters_store;
GRANT SELECT ON mmorpg_characters.migrations TO mmorpg_characters_store;
GRANT SELECT, INSERT, UPDATE ON mmorpg_characters.entries TO mmorpg_characters_store;
