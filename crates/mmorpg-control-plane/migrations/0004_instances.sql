-- The party-private instance run directory (#404 part 2, ADR 0009): one row, read and
-- rewritten under its row lock, so every host's run admissions are linearized.
CREATE TABLE mmorpg_coordination.instance_directory (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    state bytea NOT NULL
);
INSERT INTO mmorpg_coordination.instance_directory (singleton, state) VALUES (true, '');
