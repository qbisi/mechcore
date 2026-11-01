"""Test tournament filtering and safe resume without a game.

Run: uv run --with pyyaml python scripts/corpus/test_tournament_replays.py
"""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('tournament_replays', Path(__file__).with_name('tournament-replays.py'))
collector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collector)


class TournamentFiltering(unittest.TestCase):
    def setUp(self):
        self.room = tempfile.TemporaryDirectory()
        self.addCleanup(self.room.cleanup)
        self.output = Path(self.room.name)
        self.path = 'server/replay.rep'
        (self.output / '.downloads').mkdir()
        self.native = self.output / '.downloads' / (hashlib.sha256(self.path.encode()).hexdigest() + '.grbr')
        self.native.write_text('<?xml version="1.0"?><BattleRecord><BattleInfo><BattleID>battle</BattleID>'
                               '<MaxRound>13</MaxRound></BattleInfo><playerRecords>'
                               '<PlayerRecord><id>1</id></PlayerRecord>'
                               '<PlayerRecord><id>2</id></PlayerRecord></playerRecords></BattleRecord>')
        self.scores = [{'player_id': '1', 'reactor_core': 402, 'opponent_reactor_core': 12, 'win': True}]

    def test_round_limit_remains_collectible_and_early_termination_is_filtered(self):
        for round_number, expected in [(12, 'early_termination'), (13, None)]:
            match = self.output / 'battle.yaml'
            original = f'kind: action\nround: {round_number}\nblue: []\nred: []\n'
            match.write_text(original)
            result, reason = collector.append_result(match, self.native, self.scores)
            self.assertEqual(reason, expected)
            self.assertEqual('kind: result' in match.read_text(), expected is None)
            self.assertEqual(result['blue']['reactor_core'], 402)
        destroyed = collector.final_result(self.native, [dict(self.scores[0], opponent_reactor_core=-12)], 12)
        self.assertIsNone(collector.exclusion_reason(self.native, destroyed))

    def test_resume_filters_only_an_unchanged_generated_match(self):
        destination = self.output / 'battle.yaml'
        destination.write_text('generated match')
        result = collector.final_result(self.native, self.scores, 12)
        manifest = {self.path: {'battle_id': 'battle', 'native': self.native.name,
                               'match': destination.name, 'result': result,
                               'sha256': hashlib.sha256(destination.read_bytes()).hexdigest()}}
        destination.write_text('user edit')
        with self.assertRaisesRegex(RuntimeError, 'changed'):
            collector.download_one(None, self.path, self.output, Path('unused'), manifest, self.scores)
        self.assertEqual(destination.read_text(), 'user edit')
        destination.write_text('generated match')
        answer = collector.download_one(None, self.path, self.output, Path('unused'), manifest, self.scores)
        self.assertEqual(answer['skipped'], 'early_termination')
        self.assertFalse(destination.exists())
        self.assertTrue(self.native.exists())
        self.assertEqual(manifest[self.path]['scores'], self.scores)

    def test_opening_only_refusal_is_a_retained_skip(self):
        error = {'schema': 'mechcore.error', 'kind': 'refused', 'operation': 'convert',
                 'reason': 'replay holds 1 round, which is the opening alone, and a match is deployment rounds'}
        with patch.object(collector.subprocess, 'run', return_value=SimpleNamespace(returncode=1, stderr=json.dumps(error))):
            manifest = {}
            answer = collector.download_one(None, self.path, self.output, Path('unused'), manifest, self.scores)
        self.assertEqual(answer['skipped'], 'opening_only')
        self.assertTrue(self.native.exists())
        self.assertEqual(manifest[self.path]['scores'], self.scores)


if __name__ == '__main__':
    unittest.main()
