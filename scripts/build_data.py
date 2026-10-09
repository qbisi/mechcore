"""The build's data, as `scripts/decomp/decompile.py` exports it, for the extract scripts.

Every `scripts/extract*.py` reads the game's tables through this module and
nothing else: no path id, no hand-written field parser, no build number of
its own. The build is the version `GAME_VERSION` names unless a script is
given `--build`, and its export is `work/decomp/<build>/`, which
`scripts/decomp/decomp.py sync` fetches and `scripts/decomp/decompile.py` makes.

    container()            ConfigDataContainer.m_Structure
    level0("Class")        the m_Structure of a level0 data object, e.g. MechSkillGroupData
    shared("Class")        {name: m_Structure} of a class sharedassets0 holds, e.g. MapLayout
    names("Table")         {row id: {"en": ..., "zh": ...}} from the configuration's localization

Every row these return has its `name` in the game's English, or none: a
row's own `name` is the developers' Chinese label, and the localization's
English name replaces it. A row the game never shows, such as an internal
buff, has no English name, and its `name` is dropped. `name_lines` and
`name_field` write a row's name, and nothing for a row without one.
    description("Table", row)   a row's English description, its {n} placeholders filled from descParams
    in_standard(row)       whether a row can appear outside Interstellar Expedition
"""

import argparse
import functools
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
DECOMP = ROOT / "work" / "decomp"
# `limitedScene` values that are Interstellar Expedition (`matchSettings` with
# `serverSubType` 8 and 9). mechcore models no scene of that mode.
EXPEDITION_SCENES = {8, 9}


def configured_build():
    version = (ROOT / "GAME_VERSION").read_text().strip()
    if not re.fullmatch(r"[0-9]+(\.[0-9]+)+", version):
        sys.exit(f"GAME_VERSION names no game version ({version!r})")
    return version


def arguments(description, extra=None):
    """Parse `--build` and whatever else a script adds; select the build."""
    parser = argparse.ArgumentParser(description=description,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--build", default=None, help="a build under work/decomp; GAME_VERSION's by default")
    if extra:
        extra(parser)
    parsed = parser.parse_args()
    select(parsed.build or configured_build())
    return parsed


_build = None


def select(build):
    global _build
    directory = DECOMP / build
    if not (directory / "config-data-container.json").exists():
        sys.exit(
            f"no export of build {build} under {DECOMP}; run "
            f"`scripts/decomp/decomp.py sync --build {build}`, or `scripts/decomp/decompile.py` on the machine with the game"
        )
    _build = build
    _load.cache_clear()


def build():
    if _build is None:
        select(configured_build())
    return _build


@functools.cache
def _load(relative):
    path = DECOMP / build() / relative
    if not path.exists():
        sys.exit(f"build {build()} has no {relative}; rerun `scripts/decomp/decompile.py --force config`")
    return json.loads(path.read_text())


def container():
    return _in_english(_load("config-data-container.json")["m_Structure"])


def level0(name):
    return _in_english(_load(f"level0/{name}.json")["m_Structure"])


def shared(name):
    directory = DECOMP / build() / "sharedassets0" / name
    if directory.is_dir():
        return {path.stem: _load(f"sharedassets0/{name}/{path.name}")["m_Structure"]
                for path in sorted(directory.glob("*.json"))}
    return {name: _load(f"sharedassets0/{name}.json")["m_Structure"]}


@functools.cache
def _terms():
    source = _load("resources/LanguageSourceAsset.json")["m_Structure"]["mSource"]
    languages = [language["Name"] for language in source["mLanguages"]]
    english, chinese = languages.index("English"), languages.index("简体中文")
    return {term["Term"]: (term["Languages"][english], term["Languages"][chinese]) for term in source["mTerms"]}


def names(table):
    """The official English and Simplified Chinese name of every row of a config table."""
    prefix = f"ConfigData/{table}/name_"
    return {
        int(term[len(prefix):]): {"en": english, "zh": chinese}
        for term, (english, chinese) in _terms().items()
        if term.startswith(prefix) and term[len(prefix):].isdigit()
    }


@functools.cache
def _english_by_row():
    """{(row id, Chinese name): English name} over every table's name terms."""
    found = {}
    for term, (english, chinese) in _terms().items():
        match = re.fullmatch(r"ConfigData/\w+/name_(\d+)", term)
        if match and english:
            key = (int(match.group(1)), chinese)
            if found.get(key, english) != english:
                sys.exit(f"row {key[0]} named {chinese!r} has two English names")
            found[key] = english
    return found


def _in_english(value):
    """`value` with every row's `name` replaced by its English one, or dropped.

    The English name is the localization term whose row id and Chinese text
    are the row's: the two together pick one term.
    """
    if isinstance(value, list):
        return [_in_english(item) for item in value]
    if not isinstance(value, dict):
        return value
    value = {key: _in_english(item) for key, item in value.items()}
    if isinstance(value.get("id"), int) and isinstance(value.get("name"), str):
        english = _english_by_row().get((value["id"], value["name"]))
        if english:
            value["name"] = english
        else:
            del value["name"]
    return value


def _scalar(text):
    """A name as a YAML scalar, quoted only where YAML would read it otherwise."""
    return json.dumps(text, ensure_ascii=False) if re.search(r"[:#{}\[\],&*!|>%@`\"]|^\s|\s$", text) else text


def name_lines(row, indent):
    """The block-style `name:` line of a row, or none for a row without one."""
    return [f"{indent}name: {_scalar(row['name'])}"] if row.get("name") else []


def name_field(row):
    """The flow-style `name: ..., ` of a row, or nothing for a row without one."""
    return f"name: {_scalar(row['name'])}, " if row.get("name") else ""


def description(table, row):
    """A row's English description as the game shows it: `{n}` is the n-th of `descParams`."""
    text = _terms().get(f"ConfigData/{table}/description_{row['id']}", ("", ""))[0]
    params = [param for param in (row.get("descParams") or "").split(";")]
    return re.sub(r"\{(\d+)\}", lambda match: params[int(match.group(1))] if int(match.group(1)) < len(params) else match.group(0), text)


def in_standard(row):
    """A row limited to Expedition scenes only is not part of any mode mechcore models."""
    scenes = row.get("limitedScene")
    return not scenes or not set(scenes) <= EXPEDITION_SCENES


def header(generator):
    """The first lines of a generated config file."""
    return [f"# Generated by {generator}; do not edit by hand."]
