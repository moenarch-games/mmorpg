-- The party directory (#404, ADR 0008): one row holding the whole directory, serialized and
-- rewritten under its row lock so every operation of every host is linearized. Trusted-LAN
-- scale; a per-party schema is future work if the parties ever outgrow one row.
CREATE TABLE mmorpg_coordination.party_directory (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    state bytea NOT NULL
);
INSERT INTO mmorpg_coordination.party_directory (singleton, state) VALUES (true, '');
