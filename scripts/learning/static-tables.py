"""Generate static model catalogues from config only.

uv run --with pyyaml python scripts/learning/static-tables.py [--output DIR] [--check]
"""
import argparse
import hashlib
import json
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
# Source encodings are defined by config/technology_effects.yaml and its
# extractor. Do not infer fixed point from magnitude or a suffix.
Q32_TOP = set('''additional_damage_rate air_damage_change_rate all_weapon_reduce_damage_rate
attack_interval_rate attack_interval_value attack_range_value attack_range_rate
burrow_amplify_damage_rate burrow_relieve_distance chain_damage_rate chain_delay
chain_preferred_range chain_select_range cloak_delay cloak_exit_delay control_recovery_rate
damage_rate dead_explosion_multiplier dead_explosion_range exit_time_rate exp_rate
fire_life_time fire_range fly_landing_duration fog_attack_range_rate ground_damage_change_rate
group_damage_rate kill_explosion_range life_rate lifesteal_multiplier melee_damage_rate
melee_life_rate probability projectile_duration_value projectile_life_rate
projectile_random_range_value projectile_speed_value range_item_life_time range_item_range
reactive_armor_rate recovery_duration recovery_life_rate reduce_damage_from_remote
repair_interval repair_max_life_rate repair_range rvo_move_radius rvo_near_target_threshold
secondary_splash_range share_distance spawn_shield_radius splash_range splash_range_value
start_time strike_attack_point strike_damage_rate strike_splash_range sweep_length_value
sweep_width_value'''.split())
Q32_NESTED = {
    'buff': set('''additive_condition_param amplify_damage_rate attack_range_rate damage_rate
        duration life_change_rate max_life_rate speed_rate step_time'''.split()),
    'buff_cycle': set('delay interval range'.split()),
    'buff_range_item': {'life'},
    'intercept': set('''cooling_time decline interval judgment_probability lower_limit
        prepare_time range_max range_min rise rise_interval'''.split()),
    'production': set('''start_time product_time create_duration unit_life_rate
        unit_damage_rate unit_attack_range_value'''.split()),
    'move_summon': set('start_time product_time create_duration'.split()),
    'positions': {'x', 'z'},
    'rebirth': set('''follow_rate follow_rotate_rate follow_start_offset_range
        interval_from_mech_center_z interval_x interval_z per_r random_offset_range
        speed_in_transfer transfer_distance_min'''.split()),
    'siege_mode': set('''animation_delay attack_interval_rate attack_interval_value
        attack_range_rate attack_range_value damage_rate duration life_rate
        projectile_speed_value splash_range_value'''.split()),
    'stealth': {'duration', 'trigger_life_rate'},
    'wreckage': {'time'},
}
NATIVE_TOP = set('''air_target_score_offset auto_recovery_state_type barrier_energy barrier_radius
buff_targets buff_trigger burrow_enters_underground chain_count clear_radius clear_range_item_types
dead_explosion_damage dead_explosion_hits_allies dead_line_ignores_shield dead_line_value
extra_skill_effect ground_target_score_offset group_max_count group_purpose ignore_electric_effect
ignored_buff_effect ignores_buff_effect inverse_lock_target kill_explosion_buffed kill_explosion_chains
kill_explosion_damage kill_explosion_hits_allies main_skill_effect melee_ammo melee_extra_ammo
melee_recovers_life melee_recovery_ignores_disable melee_skill melee_speed_value min_attack_range_value
projectile_count_value range_item_move_type range_item_time reactive_armor_count reduce_damage_value
repair_air repair_enemy repair_life repair_only_mech search_target_type secondary_buff_id
secondary_buffed secondary_damage secondary_disables_technology secondary_hits_main_target
spawn_shield_attacks spawn_shield_attacks_increment spawn_shield_energy spawn_shield_max speed_value
strike_trigger_count sweep_fixed_direction sweep_perpendicular sweep_reverse sweep_skill_id'''.split())
NATIVE_NESTED = {
    'buff': set('''additive additive_condition additive_effect additive_reset_condition attack_range_value
        clear_when_technologies_disabled debuff disable_recover disable_technology divide id invincible
        max_additive_stack summon_level_inherit summon_unit'''.split()),
    'buff_cycle': set('distance_type fly_type target_radius update_model'.split()),
    'buff_range_item': {'kind', 'range', 'rounds'},
    'dead_acid': {'range', 'rounds'},
    'dead_summon': {'unit_count', 'unit_id', 'unit_level'},
    'intercept': {'attack', 'preemptive', 'weapon_count'},
    'production': set('''appear_type create_count_per_time inherit_technology intensify_mode max_alive
        max_batch max_create_count position_space support_unit_id unit_level'''.split()),
    'move_summon': set('''appear_type create_count_per_time max_alive max_batch summon_time
        support_unit_id unit_level'''.split()),
    'rebirth': set('cost_time count follow_others rebirthing_time unit_count unit_id'.split()),
    'wreckage': {'distance'},
}


def encoded_parameters(value, encodings, path='', parent='', field=''):
    if isinstance(value, dict):
        return {k: encoded_parameters(v, encodings, f'{path}.{k}' if path else k,
                                     field, k) for k, v in sorted(value.items())}
    if isinstance(value, list):
        return [encoded_parameters(v, encodings, path + '[]', parent, field) for v in value]
    fixed = field in (Q32_NESTED.get(parent, set()) if parent else Q32_TOP)
    native = field in (NATIVE_NESTED.get(parent, set()) if parent else NATIVE_TOP)
    if not fixed and not native:
        raise ValueError(f'unclassified parameter encoding: {path}')
    encoding = 'q32.32' if fixed else 'boolean' if isinstance(value, bool) else 'native'
    if path in encodings and encodings[path] != encoding:
        raise ValueError(f'inconsistent encoding: {path}')
    encodings[path] = encoding
    if fixed:
        if isinstance(value, bool) or not isinstance(value, int):
            raise ValueError(f'expected fixed-point integer: {path}')
        return value / (1 << 32)
    return value


def unique(rows, key):
    result = {}
    for row in rows:
        identity = row[key]
        if identity in result:
            raise ValueError(f'duplicate {key}: {identity}')
        result[identity] = row
    return result


def weapon(attack, **identity):
    # These fields in units/*.yaml already use metres, seconds and ordinary
    # numeric damage. Keep burst/group semantics instead of inventing DPS.
    fields = ('base_damage', 'range', 'min_range', 'attack_half_angle', 'targets',
              'timing', 'splash_radius', 'projectile_strength', 'weapons',
              'melee', 'crosses_shields', 'quick_switch_target', 'lock_target', 'self_splash')
    path_fields = ('type', 'count', 'release_interval', 'speed', 'target_offset_radius',
                   'interceptible', 'max_life', 'damage_multipliers', 'select_radius',
                   'target_count', 'pre_flight_height', 'extra_search_range')
    return {**identity, **{k: attack[k] for k in fields if k in attack},
            'path': {k: attack['path'][k] for k in path_fields if k in attack.get('path', {})}}


def generate(root=ROOT):
    sources = {}
    def read(relative):
        raw = (root / relative).read_bytes()
        sources[relative] = hashlib.sha256(raw).hexdigest()
        return yaml.safe_load(raw)
    version = read('GAME_VERSION')
    prices = unique(read('config/unit_prices.yaml')['units'], 'type')
    catalogue = unique(read('config/unit_techs.yaml')['units'], 'type')
    names = read('config/names.yaml')['technologies']
    effects = unique(read('config/technology_effects.yaml')['technologies'], 'id')
    configs = [read(str(p.relative_to(root))) for p in sorted((root/'config/units').glob('*.yaml'))]
    configs = unique(configs, 'type_name')
    units, technologies, encodings = [], [], {}
    for name, config in sorted(configs.items()):
        fields = ('formation', 'domain', 'size', 'max_life', 'collision_radius',
                  'move_speed', 'rotate_speed', 'has_body', 'free_move', 'independent_aim')
        weapons = [weapon(config['attack'], role='main', skill_id=config['main_skill'])]
        for extra in config.get('extra_weapons', []):
            weapons.append(weapon(extra['attack'], role='extra', skill_id=extra['skill'], **{k: extra[k] for k in
                ('technology', 'use_main_skill_range', 'damage_by_level', 'damage_rate',
                 'shield_damage') if k in extra}))
        price = prices.get(name)
        base_price = None if price is None else {k: price[k] for k in
            ('supply', 'unlock_supply', 'upgrade_supply') if k in price}
        techs = catalogue.get(name, {}).get('technologies', [])
        units.append({'unit': name, 'unit_id': config['unit_type_id'],
                      **{k: config[k] for k in fields if k in config},
                      'base_price': base_price, 'technology_ids': [t['id'] for t in techs],
                      'weapons': weapons})
        for tech in techs:
            effect = effects[tech['id']]
            if effect['unit'] != name:
                raise ValueError(f'technology owner mismatch: {tech["id"]}')
            params = {k: v for k, v in effect.items() if k not in ('id', 'unit', 'name', 'kind')}
            technologies.append({'unit': name, 'technology_id': tech['id'],
                'name': names[name][tech['id']], 'kind': effect['kind'],
                'base_supply': tech['supply'],
                'parameters': encoded_parameters(params, encodings)})
    if set(catalogue) - set(configs) or set(prices) - set(configs):
        raise ValueError('catalogue unit has no combat config')
    if {t['technology_id'] for t in technologies} != set(effects):
        raise ValueError('technology catalogue and effect coverage differ')
    def dump(value):
        return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False, allow_nan=False)+'\n').encode()
    outputs = {
        'units.json': dump({'schema': 'mechcore.model.units.v1', 'units': units}),
        'technologies.json': dump({'schema': 'mechcore.model.technologies.v1',
            'parameter_encodings': encodings, 'technologies': technologies}),
    }
    outputs['manifest.json'] = dump({'schema': 'mechcore.model.static.v1', 'game_version': version,
        'sources': sources, 'tables': {k: hashlib.sha256(v).hexdigest() for k,v in outputs.items()},
        'counts': {'units': len(units), 'technologies': len(technologies)}})
    return outputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'work/learning/static')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    outputs = generate()
    if args.check:
        stale = [name for name, data in outputs.items()
                 if not (args.output/name).is_file() or (args.output/name).read_bytes() != data]
        if stale:
            parser.exit(1, 'stale or missing: '+', '.join(stale)+'\n')
    else:
        args.output.mkdir(parents=True, exist_ok=True)
        for name, data in outputs.items():
            (args.output/name).write_bytes(data)
    print(json.loads(outputs['manifest.json'])['counts'])


if __name__ == '__main__':
    main()
