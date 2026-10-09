-- Every event from tick :from to tick :to, in the order the recording keeps them.
SELECT tick, ordinal, type,
       object__kind, object__id, source__kind, source__id, target__kind, target__id,
       amount, skill_slot, buff_id, team_id, unit_type_id
FROM events
WHERE tick BETWEEN :from AND :to
ORDER BY tick, ordinal
