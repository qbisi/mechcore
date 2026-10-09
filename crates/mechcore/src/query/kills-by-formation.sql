-- Each formation's kills and damage at the fight's last tick, as the build
-- counts them, with the placement and unit type the layout gives it, most
-- kills first.
SELECT s.team_id, s.recorder_id AS formation_id, l.placement, l.name,
       s.kills, s.damage, s.damage_real, s.damage_taken
FROM statistics s
LEFT JOIN layout_units l ON l.formation_id = s.recorder_id
WHERE s.recorder = 'formation' AND s.tick = (SELECT max(tick) FROM statistics)
ORDER BY s.kills DESC, s.damage DESC, s.team_id, s.recorder_id
