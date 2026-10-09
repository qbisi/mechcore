-- Of two recordings attached as left and right, every table that differs:
-- the first tick a row of it differs on and how many ticks do, first
-- divergence first. A row that only one side holds differs.
SELECT * FROM (
SELECT 'ticks' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.ticks EXCEPT SELECT * FROM right.ticks)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.ticks EXCEPT SELECT * FROM left.ticks))
UNION ALL
SELECT 'units' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.units EXCEPT SELECT * FROM right.units)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.units EXCEPT SELECT * FROM left.units))
UNION ALL
SELECT 'units__buffs' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.units__buffs EXCEPT SELECT * FROM right.units__buffs)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.units__buffs EXCEPT SELECT * FROM left.units__buffs))
UNION ALL
SELECT 'units__skills' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.units__skills EXCEPT SELECT * FROM right.units__skills)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.units__skills EXCEPT SELECT * FROM left.units__skills))
UNION ALL
SELECT 'units__skills__enabled__weapons' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.units__skills__enabled__weapons EXCEPT SELECT * FROM right.units__skills__enabled__weapons)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.units__skills__enabled__weapons EXCEPT SELECT * FROM left.units__skills__enabled__weapons))
UNION ALL
SELECT 'units__control__sources' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.units__control__sources EXCEPT SELECT * FROM right.units__control__sources)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.units__control__sources EXCEPT SELECT * FROM left.units__control__sources))
UNION ALL
SELECT 'rebirths' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.rebirths EXCEPT SELECT * FROM right.rebirths)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.rebirths EXCEPT SELECT * FROM left.rebirths))
UNION ALL
SELECT 'projectiles' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.projectiles EXCEPT SELECT * FROM right.projectiles)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.projectiles EXCEPT SELECT * FROM left.projectiles))
UNION ALL
SELECT 'projectiles__spawn_containing_shields' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.projectiles__spawn_containing_shields EXCEPT SELECT * FROM right.projectiles__spawn_containing_shields)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.projectiles__spawn_containing_shields EXCEPT SELECT * FROM left.projectiles__spawn_containing_shields))
UNION ALL
SELECT 'buildings' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.buildings EXCEPT SELECT * FROM right.buildings)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.buildings EXCEPT SELECT * FROM left.buildings))
UNION ALL
SELECT 'shields' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.shields EXCEPT SELECT * FROM right.shields)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.shields EXCEPT SELECT * FROM left.shields))
UNION ALL
SELECT 'terrains' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.terrains EXCEPT SELECT * FROM right.terrains)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.terrains EXCEPT SELECT * FROM left.terrains))
UNION ALL
SELECT 'terrains__grid__rows' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.terrains__grid__rows EXCEPT SELECT * FROM right.terrains__grid__rows)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.terrains__grid__rows EXCEPT SELECT * FROM left.terrains__grid__rows))
UNION ALL
SELECT 'terrains__applications' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.terrains__applications EXCEPT SELECT * FROM right.terrains__applications)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.terrains__applications EXCEPT SELECT * FROM left.terrains__applications))
UNION ALL
SELECT 'statistics' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.statistics EXCEPT SELECT * FROM right.statistics)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.statistics EXCEPT SELECT * FROM left.statistics))
UNION ALL
SELECT 'formations' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.formations EXCEPT SELECT * FROM right.formations)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.formations EXCEPT SELECT * FROM left.formations))
UNION ALL
SELECT 'events' AS "table", min(tick) AS first_tick, count(DISTINCT tick) AS ticks FROM (
  SELECT tick FROM (SELECT * FROM left.events EXCEPT SELECT * FROM right.events)
  UNION ALL
  SELECT tick FROM (SELECT * FROM right.events EXCEPT SELECT * FROM left.events))
) WHERE first_tick IS NOT NULL
ORDER BY first_tick, "table"
