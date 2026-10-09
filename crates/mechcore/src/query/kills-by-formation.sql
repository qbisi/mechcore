-- Each formation's kills and damage at the fight's last tick, as the build
-- counts them, most kills first.
SELECT team_id, recorder_id AS formation_id, kills, damage, damage_real, damage_taken
FROM statistics
WHERE recorder = 'formation' AND tick = (SELECT max(tick) FROM statistics)
ORDER BY kills DESC, damage DESC, team_id, formation_id
