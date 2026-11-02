"""Run with uv run --with pyyaml python scripts/learning/test_static_tables.py."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('static_tables', Path(__file__).with_name('static-tables.py'))
TABLES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TABLES)


class StaticTablesTest(unittest.TestCase):
    def test_mixed_encodings_and_rank_lists(self):
        encodings = {}
        parameters = TABLES.encoded_parameters({
            'life_rate': [1 << 31, 1 << 32], 'speed_value': [3],
            'min_attack_range_value': [75], 'barrier_radius': [50],
            'reduce_damage_value': [120], 'attack_range_value': [20 << 32],
            'buff': {'duration': 2 << 32, 'attack_range_value': 10},
            'production': {'positions': [{'x': -(20 << 32), 'z': 0}], 'unit_level': 3},
            'rebirth': {'cost_time': 5, 'rebirthing_time': 1.5},
        }, encodings)
        self.assertEqual(parameters['life_rate'], [0.5, 1.0])
        self.assertEqual(parameters['speed_value'], [3])
        self.assertEqual(parameters['min_attack_range_value'], [75])
        self.assertEqual(parameters['barrier_radius'], [50])
        self.assertEqual(parameters['reduce_damage_value'], [120])
        self.assertEqual(parameters['attack_range_value'], [20.0])
        self.assertEqual(parameters['buff'], {'duration': 2.0, 'attack_range_value': 10})
        self.assertEqual(parameters['production']['positions'][0]['x'], -20.0)
        self.assertEqual(parameters['rebirth']['rebirthing_time'], 1.5)
        self.assertEqual(encodings['life_rate[]'], 'q32.32')
        self.assertNotIn('damage_rate', parameters)
        with self.assertRaisesRegex(ValueError, 'unclassified'):
            TABLES.encoded_parameters({'unknown_future_parameter': 1 << 32}, {})

    def test_catalogue_joins_weapons_and_provenance(self):
        first = TABLES.generate()
        self.assertEqual(first, TABLES.generate())
        units = {u['unit']: u for u in json.loads(first['units.json'])['units']}
        techs = json.loads(first['technologies.json'])['technologies']
        self.assertEqual(len({t['technology_id'] for t in techs}), len(techs))
        for t in techs:
            self.assertIn(t['technology_id'], units[t['unit']]['technology_ids'])
        self.assertTrue(any(u['base_price'] is None for u in units.values()))
        self.assertTrue(any(len(u['weapons']) > 1 for u in units.values()))
        self.assertEqual(units['wraith']['weapons'][0]['weapons']['mode'], 'group')
        manifest = json.loads(first['manifest.json'])
        for name, digest in manifest['tables'].items():
            self.assertEqual(hashlib.sha256(first[name]).hexdigest(), digest)
        for name, digest in manifest['sources'].items():
            self.assertEqual(hashlib.sha256((TABLES.ROOT/name).read_bytes()).hexdigest(), digest)

    def test_check_detects_staleness_without_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            outputs = TABLES.generate()
            for name, data in outputs.items():
                (Path(directory)/name).write_bytes(data)
            command = [sys.executable, str(Path(__file__).with_name('static-tables.py')),
                       '--output', directory, '--check']
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
            stale = Path(directory)/'units.json'
            stale.write_text('{}\n')
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn('units.json', result.stderr)
            self.assertEqual(stale.read_text(), '{}\n')


if __name__ == '__main__':
    unittest.main()
