-- Of two recordings attached as left and right, every column of every unit
-- that differs at tick :tick, with each side's stored value; a unit only one
-- side holds has its other side's values null.
WITH joined AS (
  SELECT coalesce(l.unit_id, r.unit_id) AS unit_id,
         l.team_id AS left_team_id, r.team_id AS right_team_id,
         l.original_team_id AS left_original_team_id, r.original_team_id AS right_original_team_id,
         l.formation_id AS left_formation_id, r.formation_id AS right_formation_id,
         l.unit_type_id AS left_unit_type_id, r.unit_type_id AS right_unit_type_id,
         l.domain AS left_domain, r.domain AS right_domain,
         l.position__x AS left_position__x, r.position__x AS right_position__x,
         l.position__y AS left_position__y, r.position__y AS right_position__y,
         l.position__z AS left_position__z, r.position__z AS right_position__z,
         l.body_rotation AS left_body_rotation, r.body_rotation AS right_body_rotation,
         l.turret_rotation AS left_turret_rotation, r.turret_rotation AS right_turret_rotation,
         l.velocity__x AS left_velocity__x, r.velocity__x AS right_velocity__x,
         l.velocity__z AS left_velocity__z, r.velocity__z AS right_velocity__z,
         l.motion_state AS left_motion_state, r.motion_state AS right_motion_state,
         l.has_mech_lock_target AS left_has_mech_lock_target, r.has_mech_lock_target AS right_has_mech_lock_target,
         l.mech_lock_target__kind AS left_mech_lock_target__kind, r.mech_lock_target__kind AS right_mech_lock_target__kind,
         l.mech_lock_target__id AS left_mech_lock_target__id, r.mech_lock_target__id AS right_mech_lock_target__id,
         l.collision_radius AS left_collision_radius, r.collision_radius AS right_collision_radius,
         l.life__current AS left_life__current, r.life__current AS right_life__current,
         l.life__maximum AS left_life__maximum, r.life__maximum AS right_life__maximum,
         l.active AS left_active, r.active AS right_active,
         l.targetable AS left_targetable, r.targetable AS right_targetable,
         l.visibility AS left_visibility, r.visibility AS right_visibility,
         l.personal_shield__active AS left_personal_shield__active, r.personal_shield__active AS right_personal_shield__active,
         l.personal_shield__enabled AS left_personal_shield__enabled, r.personal_shield__enabled AS right_personal_shield__enabled,
         l.personal_shield__energy__current AS left_personal_shield__energy__current, r.personal_shield__energy__current AS right_personal_shield__energy__current,
         l.personal_shield__energy__maximum AS left_personal_shield__energy__maximum, r.personal_shield__energy__maximum AS right_personal_shield__energy__maximum,
         l.move_speed AS left_move_speed, r.move_speed AS right_move_speed,
         l.has_control AS left_has_control, r.has_control AS right_has_control,
         l.control__progress AS left_control__progress, r.control__progress AS right_control__progress,
         l.rebirth_count AS left_rebirth_count, r.rebirth_count AS right_rebirth_count
  FROM (SELECT * FROM left.units WHERE tick = :tick) l
  FULL OUTER JOIN (SELECT * FROM right.units WHERE tick = :tick) r USING (unit_id)
)
SELECT unit_id, "column", left, right FROM (
  SELECT unit_id, 'present' AS "column", left_unit_type_id IS NOT NULL AS left, right_unit_type_id IS NOT NULL AS right
  FROM joined WHERE left_unit_type_id IS NULL OR right_unit_type_id IS NULL
  UNION ALL SELECT unit_id, 'team_id', left_team_id, right_team_id FROM joined WHERE left_team_id IS NOT right_team_id
  UNION ALL SELECT unit_id, 'original_team_id', left_original_team_id, right_original_team_id FROM joined WHERE left_original_team_id IS NOT right_original_team_id
  UNION ALL SELECT unit_id, 'formation_id', left_formation_id, right_formation_id FROM joined WHERE left_formation_id IS NOT right_formation_id
  UNION ALL SELECT unit_id, 'unit_type_id', left_unit_type_id, right_unit_type_id FROM joined WHERE left_unit_type_id IS NOT right_unit_type_id
  UNION ALL SELECT unit_id, 'domain', left_domain, right_domain FROM joined WHERE left_domain IS NOT right_domain
  UNION ALL SELECT unit_id, 'position__x', left_position__x, right_position__x FROM joined WHERE left_position__x IS NOT right_position__x
  UNION ALL SELECT unit_id, 'position__y', left_position__y, right_position__y FROM joined WHERE left_position__y IS NOT right_position__y
  UNION ALL SELECT unit_id, 'position__z', left_position__z, right_position__z FROM joined WHERE left_position__z IS NOT right_position__z
  UNION ALL SELECT unit_id, 'body_rotation', left_body_rotation, right_body_rotation FROM joined WHERE left_body_rotation IS NOT right_body_rotation
  UNION ALL SELECT unit_id, 'turret_rotation', left_turret_rotation, right_turret_rotation FROM joined WHERE left_turret_rotation IS NOT right_turret_rotation
  UNION ALL SELECT unit_id, 'velocity__x', left_velocity__x, right_velocity__x FROM joined WHERE left_velocity__x IS NOT right_velocity__x
  UNION ALL SELECT unit_id, 'velocity__z', left_velocity__z, right_velocity__z FROM joined WHERE left_velocity__z IS NOT right_velocity__z
  UNION ALL SELECT unit_id, 'motion_state', left_motion_state, right_motion_state FROM joined WHERE left_motion_state IS NOT right_motion_state
  UNION ALL SELECT unit_id, 'has_mech_lock_target', left_has_mech_lock_target, right_has_mech_lock_target FROM joined WHERE left_has_mech_lock_target IS NOT right_has_mech_lock_target
  UNION ALL SELECT unit_id, 'mech_lock_target__kind', left_mech_lock_target__kind, right_mech_lock_target__kind FROM joined WHERE left_mech_lock_target__kind IS NOT right_mech_lock_target__kind
  UNION ALL SELECT unit_id, 'mech_lock_target__id', left_mech_lock_target__id, right_mech_lock_target__id FROM joined WHERE left_mech_lock_target__id IS NOT right_mech_lock_target__id
  UNION ALL SELECT unit_id, 'collision_radius', left_collision_radius, right_collision_radius FROM joined WHERE left_collision_radius IS NOT right_collision_radius
  UNION ALL SELECT unit_id, 'life__current', left_life__current, right_life__current FROM joined WHERE left_life__current IS NOT right_life__current
  UNION ALL SELECT unit_id, 'life__maximum', left_life__maximum, right_life__maximum FROM joined WHERE left_life__maximum IS NOT right_life__maximum
  UNION ALL SELECT unit_id, 'active', left_active, right_active FROM joined WHERE left_active IS NOT right_active
  UNION ALL SELECT unit_id, 'targetable', left_targetable, right_targetable FROM joined WHERE left_targetable IS NOT right_targetable
  UNION ALL SELECT unit_id, 'visibility', left_visibility, right_visibility FROM joined WHERE left_visibility IS NOT right_visibility
  UNION ALL SELECT unit_id, 'personal_shield__active', left_personal_shield__active, right_personal_shield__active FROM joined WHERE left_personal_shield__active IS NOT right_personal_shield__active
  UNION ALL SELECT unit_id, 'personal_shield__enabled', left_personal_shield__enabled, right_personal_shield__enabled FROM joined WHERE left_personal_shield__enabled IS NOT right_personal_shield__enabled
  UNION ALL SELECT unit_id, 'personal_shield__energy__current', left_personal_shield__energy__current, right_personal_shield__energy__current FROM joined WHERE left_personal_shield__energy__current IS NOT right_personal_shield__energy__current
  UNION ALL SELECT unit_id, 'personal_shield__energy__maximum', left_personal_shield__energy__maximum, right_personal_shield__energy__maximum FROM joined WHERE left_personal_shield__energy__maximum IS NOT right_personal_shield__energy__maximum
  UNION ALL SELECT unit_id, 'move_speed', left_move_speed, right_move_speed FROM joined WHERE left_move_speed IS NOT right_move_speed
  UNION ALL SELECT unit_id, 'has_control', left_has_control, right_has_control FROM joined WHERE left_has_control IS NOT right_has_control
  UNION ALL SELECT unit_id, 'control__progress', left_control__progress, right_control__progress FROM joined WHERE left_control__progress IS NOT right_control__progress
  UNION ALL SELECT unit_id, 'rebirth_count', left_rebirth_count, right_rebirth_count FROM joined WHERE left_rebirth_count IS NOT right_rebirth_count
)
ORDER BY unit_id, "column"
