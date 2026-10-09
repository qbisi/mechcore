-- How far each unit moved on the ground plane, in metres, summed over every
-- tick it stood in, with its formation's placement and unit type, farthest
-- first.
WITH steps AS (
  SELECT unit_id, team_id, formation_id, unit_type_id, tick,
         hypot(q32(position__x - lag(position__x) OVER w),
               q32(position__z - lag(position__z) OVER w)) AS step
  FROM units
  WINDOW w AS (PARTITION BY unit_id ORDER BY tick)
)
SELECT s.unit_id, s.team_id, s.formation_id, l.placement, l.name, s.unit_type_id,
       min(s.tick) AS first_tick, max(s.tick) AS last_tick,
       round(total(s.step), 3) AS distance
FROM steps s
LEFT JOIN layout_units l ON l.formation_id = s.formation_id
GROUP BY s.unit_id
ORDER BY distance DESC, s.unit_id
