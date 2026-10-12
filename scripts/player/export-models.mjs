// Export the player's sprites as SVG files into `assets/models/`.
//
//     node scripts/player/export-models.mjs [--check]
//
// The sprites are code: `crates/player/web/sprites.js` draws each unit and
// building on a canvas, its moving parts placed by a pose. This runs that file
// unchanged against a context that records the drawing as SVG instead of
// painting it, in each sprite's rest pose and the first team's colours, and
// writes `assets/models/<sprite>.svg`. Lengths stay in metres, the sprite's front up
// the page. `--check` writes nothing and fails when a file differs from what
// the sprites draw now, which is how CI holds the directory to the code.
//
// Node from 18 on, nothing to install.

import { readFileSync, writeFileSync, readdirSync, unlinkSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../..');
const SPRITES = join(ROOT, 'crates/player/web/sprites.js');
const SNAPSHOT = join(ROOT, 'crates/player/web/snapshot.js');
const OUT = join(ROOT, 'assets/models');
// The sprites the page draws, and whether each is a building.
const MODELS = [
  ['marksman', false], ['arclight', false], ['rhino', false], ['crawler', false],
  ['sledgehammer', false], ['wasp', false], ['phoenix', false], ['fang', false],
  ['hound', false],
  ['void_eye', false],
  ['vortex', false],
  ['mustang', false],
  ['steel_ball', false],
  ['stormcaller', false],
  ['hacker', false],
  ['fire_badger', false],
  ['sabertooth', false],
  ['tarantula', false],
  ['phantom_ray', false],
  ['wraith', false],
  ['scorpion', false],
  ['typhoon', false],
  ['farseer', false],
  ['centurion', false],
  ['fortress', false],
  ['vulcan', false],
  ['melting_point', false],
  ['overlord', false],
  ['war_factory', false],
  ['sandworm', false],
  ['raiden', false],
  ['abyss', false],
  ['mountain', false],
  ['larva', false],
  ['spider_mine', false],
  ['sentry_missile', false],
  ['energy_tower', true],
  ['research_center', true], ['anti_armor_turret', true], ['rapid_fire_turret', true],
  ['defensive_wall', true], ['interceptor', true],
];


const SnapshotContext = vm.runInNewContext(`${readFileSync(SNAPSHOT, 'utf8')}\nSnapshotContext;`, { Math });

function load() {
  const sandbox = { Math };
  const code = readFileSync(SPRITES, 'utf8');
  return vm.runInNewContext(`${code}\nSprites;`, sandbox, { filename: SPRITES });
}

function main() {
  const check = process.argv.includes('--check');
  const sprites = load();
  // the outline width the page draws at 10 pixels to the metre
  sprites.setScale(0.1);
  const files = new Map();
  for (const [kind, building] of MODELS) {
    // in blue, the first team's colours; the other team's only differ there
    const c = new SnapshotContext();
    const pose = sprites.pose();
    const [width, depth] = sprites.SIZE[kind];
    if (building) sprites.drawBuilding(c, kind, pose, width, depth);
    else sprites.drawUnit(c, kind, pose, 4);
    files.set(`${kind}.svg`, c.snapshot(kind));
  }
  const stale = [];
  const present = new Set(readdirSync(OUT).filter((file) => file.endsWith('.svg')));
  for (const [file, text] of files) {
    const path = join(OUT, file);
    let old = null;
    try { old = readFileSync(path, 'utf8'); } catch { /* not written yet */ }
    if (old !== text) {
      stale.push(file);
      if (!check) writeFileSync(path, text);
    }
    present.delete(file);
  }
  for (const file of present) {
    stale.push(file);
    if (!check) unlinkSync(join(OUT, file));
  }
  if (check && stale.length) {
    console.error(`assets/models/ is not what crates/player/web/sprites.js draws: ${stale.join(', ')}`);
    console.error('run: node scripts/player/export-models.mjs');
    process.exit(1);
  }
  console.log(`${files.size} models${check ? ' match the sprites' : ` written, ${stale.length} changed`}`);
}

main();
