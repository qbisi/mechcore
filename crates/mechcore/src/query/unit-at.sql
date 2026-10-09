-- One unit at one tick: where it stands, its life and motion, and its buffs
-- and skills as JSON arrays.
SELECT u.tick, u.unit_id, u.team_id, u.formation_id, l.placement, l.name, u.unit_type_id,
       q32(u.position__x) AS x, q32(u.position__z) AS z,
       u.life__current, u.life__maximum, u.motion_state, q32(u.move_speed) AS move_speed,
       (SELECT json_group_array(json_object(
                 'kind', b.data__kind, 'id', b.data__id, 'stacks', b.stacks,
                 'elapsed', b.elapsed, 'duration', b.duration,
                 'source', b.source__kind || ':' || b.source__id))
        FROM units__buffs b
        WHERE b.tick = u.tick AND b.unit_id = u.unit_id) AS buffs,
       (SELECT json_group_array(json_object(
                 'slot', k.skill_slot, 'enabled', k.has_enabled, 'state', k.enabled__state,
                 'attack_target', k.enabled__attack_target__kind || ':' || k.enabled__attack_target__id,
                 'attack_range', q32(k.enabled__attack_range),
                 'attack_damage', k.enabled__attack_damage))
        FROM units__skills k
        WHERE k.tick = u.tick AND k.unit_id = u.unit_id) AS skills
FROM units u
LEFT JOIN layout_units l ON l.formation_id = u.formation_id
WHERE u.unit_id = :unit AND u.tick = :tick
