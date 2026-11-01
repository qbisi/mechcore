"""Test tournament filtering and safe resume without a game.

Run: uv run --with pyyaml python scripts/corpus/test_tournament_replays.py
"""

import hashlib
import importlib.util
import json
import io
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch
import urllib.error


SPEC = importlib.util.spec_from_file_location('tournament_replays', Path(__file__).with_name('tournament-replays.py'))
collector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collector)


def varint(value):
    out = bytearray()
    while value >= 128:
        out.append((value & 127) | 128)
        value >>= 7
    return bytes(out) + bytes([value])


def message(number, data):
    return varint(number << 3 | 2) + varint(len(data)) + data


def raw_replay(blue=1448, red=1444):
    snapshot = message(2, message(2, b'battle'))
    for player_id, rating in [(1, blue), (2, red)]:
        base = varint(1 << 3) + varint(player_id)
        if rating is not None:
            base += varint(16 << 3) + varint(rating)
        snapshot += message(1, message(1, base))
    return message(1, snapshot) + message(5, b'test')


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
        self.native.with_suffix('.rep').write_bytes(raw_replay())
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

    def test_raw_ratings_preserve_unknown_and_explicit_zero(self):
        result = collector.server_replay.read(raw_replay(0, None))
        self.assertEqual(result['ratings'], {'1': 0, '2': None})
        for invalid in [b'', b'\x00', b'\x0a\x05\x01', b'\x80' * 11]:
            with self.assertRaises(RuntimeError):
                collector.server_replay.read(invalid)

    def test_header_labels_follow_player_ids_and_leave_rounds_unchanged(self):
        match = self.output / 'match.yaml'
        body = '---\nkind: action\nround: 1\nblue: []\nred: []\n'
        match.write_text('kind: match\nbuild: test\nblue: {}\nred: {}\n' + body)
        original = {'battle_id': 'battle', 'build': 'test', 'ratings': {'2': 2000, '1': 1000}}
        collector.label_header(match, self.native, original)
        header = collector.yaml.safe_load_all(match.read_text())
        header = next(header)
        self.assertEqual(header['blue']['elo_score'], 1000)
        self.assertEqual(header['red']['elo_score'], 2000)
        self.assertTrue(match.read_text().endswith(body))
        with self.assertRaisesRegex(RuntimeError, 'identity'):
            collector.label_header(match, self.native, dict(original, battle_id='wrong'))

    def test_original_download_falls_back_and_keeps_native_bytes(self):
        target = self.output / 'original.rep'
        missing = urllib.error.HTTPError('https://first/', 404, 'missing', {}, None)
        data = raw_replay()
        with patch.object(collector.urllib.request, 'urlopen', side_effect=[missing, io.BytesIO(data)]) as request:
            decoded = collector.download_original('test/one.rep', ['https://first/replay/', 'https://second/replay/'], target)
        self.assertEqual(request.call_count, 2)
        self.assertEqual(target.read_bytes(), data)
        self.assertEqual(decoded['ratings']['1'], 1448)

    def test_resume_refreshes_owned_labels_and_preserves_ownership_on_failure(self):
        destination = self.output / 'battle.yaml'
        original = 'kind: match\nbuild: test\nblue: {}\nred: {}\n---\nkind: action\nround: 13\nblue: []\nred: []\n'
        destination.write_text(original)
        recorded = {'battle_id': 'battle', 'native': self.native.name, 'match': destination.name,
                    'result': collector.final_result(self.native, self.scores, 13),
                    'sha256': hashlib.sha256(destination.read_bytes()).hexdigest()}
        manifest = {self.path: dict(recorded)}
        with patch.object(collector.subprocess, 'run', return_value=SimpleNamespace(returncode=1, stderr='conversion refused')):
            with self.assertRaisesRegex(RuntimeError, 'conversion failed'):
                collector.download_one(None, self.path, self.output, Path('unused'), manifest, self.scores)
        self.assertEqual(manifest[self.path], recorded)
        self.assertEqual(destination.read_text(), original)

        def command(arguments, **kwargs):
            if arguments[1] == 'convert':
                Path(arguments[-1]).write_text(original)
            return SimpleNamespace(returncode=0, stderr='')

        with patch.object(collector.subprocess, 'run', side_effect=command):
            answer = collector.download_one(None, self.path, self.output, Path('unused'), manifest, self.scores)
        self.assertFalse(answer['duplicate'])
        header = next(collector.yaml.safe_load_all(destination.read_text()))
        self.assertEqual(header['blue']['elo_score'], 1448)
        self.assertEqual(header['red']['elo_score'], 1444)
        self.assertIn('raw_sha256', manifest[self.path])


if __name__ == '__main__':
    unittest.main()
