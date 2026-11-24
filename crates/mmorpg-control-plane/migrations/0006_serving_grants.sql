-- Serving hosts log in without DDL rights (#1142): the directory role also reads and rewrites
-- the party and instance directories, which the handoff hosts share.
GRANT SELECT, UPDATE ON mmorpg_coordination.party_directory, mmorpg_coordination.instance_directory
  TO mmorpg_zone_directory;
