"""Read historical player labels from the game's original server protobuf replay.

StoreData.roundSnap[0].players[].baseInfo carries PlayerBaseInfo.userid (1)
and eloScore (16). Field numbers come from the local native protocol types;
the GRBR reconstruction does not preserve eloScore. This reader extracts only
identity and optional opening ratings, without reconstructing game actions.
"""

MAX_BYTES = 64 * 1024 * 1024


def _fields(data: bytes) -> dict:
    at = 0

    def varint():
        nonlocal at
        result = 0
        for shift in range(0, 70, 7):
            if at >= len(data):
                raise ValueError('truncated varint')
            byte = data[at]
            at += 1
            if shift == 63 and byte > 1:
                raise ValueError('varint overflow')
            result |= (byte & 127) << shift
            if byte < 128:
                return result
        raise ValueError('unterminated varint')

    fields = {}
    while at < len(data):
        tag = varint()
        number, wire = tag >> 3, tag & 7
        if not 0 < number < (1 << 29):
            raise ValueError('invalid field number')
        if wire == 0:
            value = varint()
        elif wire in (1, 2, 5):
            size = varint() if wire == 2 else 8 if wire == 1 else 4
            if at + size > len(data):
                raise ValueError('truncated field')
            value = data[at:at + size]
            at += size
        else:
            raise ValueError(f'unsupported protobuf wire type {wire}')
        fields.setdefault(number, []).append(value)
    return fields


def read(data: bytes) -> dict:
    """Return the native BattleID, build and optional scores keyed by player ID."""
    try:
        if len(data) > MAX_BYTES:
            raise ValueError('replay exceeds size limit')
        store = _fields(data)
        opening = _fields(store[1][0])
        info = _fields(opening[2][0])
        battle_id = info[2][0].decode('utf-8')
        version = store[5][0].decode('utf-8')
        if not battle_id or not version:
            raise ValueError('missing replay identity')
        ratings = {}
        for player in opening[1]:
            base = _fields(_fields(player)[1][0])
            player_id = base[1][0]
            if not isinstance(player_id, int) or player_id <= 0 or str(player_id) in ratings:
                raise ValueError('invalid or duplicate player ID')
            score = base.get(16, [None])[-1]
            if score is not None:
                if not isinstance(score, int):
                    raise ValueError('invalid eloScore wire type')
                score &= 0xffffffff
                if score >= 1 << 31:
                    score -= 1 << 32
            # A non-opening checkpoint cannot claim a match-opening rating.
            ratings[str(player_id)] = score if opening.get(3, [0])[-1] == 0 else None
        return {'battle_id': battle_id, 'build': version, 'ratings': ratings}
    except (KeyError, IndexError, UnicodeError, ValueError, TypeError, AttributeError) as error:
        raise RuntimeError(f'invalid original server replay: {error}') from error
