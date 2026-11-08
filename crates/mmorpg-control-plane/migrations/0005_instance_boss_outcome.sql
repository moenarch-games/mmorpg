-- The run directory's stored form gained the boss outcome (#404 part 3, ADR 0010). Runs are
-- short-lived and resumable only for minutes; a directory of the old layout starts empty, and
-- its characters return at the public entrance.
UPDATE mmorpg_coordination.instance_directory SET state = '';
