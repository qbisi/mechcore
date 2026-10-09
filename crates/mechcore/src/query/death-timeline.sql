-- Every unit's death in order, with its type, side and formation, the
-- placement and unit type the layout gives its formation, and the last damage
-- dealt to it on that tick.
SELECT e.tick, e.ordinal, e.object__id AS unit_id,
       COALESCE(u.unit_type_id, c.unit_type_id) AS unit_type_id,
       COALESCE(u.team_id, c.team_id) AS team_id,
       COALESCE(u.formation_id, c.formation_id) AS formation_id,
       l.placement, l.name,
       q32(e.position__x) AS x, q32(e.position__z) AS z,
       d.source__kind AS last_hit_kind, d.source__id AS last_hit_id,
       d.amount AS last_hit_amount
FROM events e
LEFT JOIN units u ON u.unit_id = e.object__id AND u.tick = e.tick - 1
LEFT JOIN events c ON c.rowid = (
    SELECT h.rowid FROM events h
    WHERE h.type = 'unit_created' AND h.object__kind = 'unit'
      AND h.object__id = e.object__id AND h.tick <= e.tick
    ORDER BY h.tick DESC, h.ordinal DESC LIMIT 1)
LEFT JOIN events d ON d.rowid = (
    SELECT h.rowid FROM events h
    WHERE h.tick = e.tick AND h.ordinal < e.ordinal AND h.type = 'damage'
      AND h.target__kind = 'unit' AND h.target__id = e.object__id
    ORDER BY h.ordinal DESC LIMIT 1)
LEFT JOIN layout_units l ON l.formation_id = COALESCE(u.formation_id, c.formation_id)
WHERE e.type = 'unit_died'
ORDER BY e.tick, e.ordinal
