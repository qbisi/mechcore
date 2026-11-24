//! Streaming observations of a speculative battle at complete logic-tick boundaries.
use super::*;
use mechcore_mcfr::{EventKind, Settlement, ShieldState, UnitNumbering};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    LazyLock,
    atomic::{AtomicBool, Ordering as AtomicOrdering},
};

/// The bounded evidence needed to read a fight into a match settlement.
pub struct SettlementEvidence {
    pub first: WorldSnapshot,
    pub last: WorldSnapshot,
    pub shields: BTreeMap<u64, ShieldState>,
    pub settlement: Settlement,
}

pub struct Observation {
    pub tick: u32,
    pub settled: bool,
    pub stop: Value,
    pub values: BTreeMap<String, Value>,
    pub evidence: Option<SettlementEvidence>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Seat {
    Blue,
    Red,
}
impl Seat {
    fn of(id: u32) -> Self {
        if id == 0 { Self::Blue } else { Self::Red }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
enum Kinds {
    One(EventKind),
    Many { r#in: Vec<EventKind> },
}
impl Kinds {
    fn matches(&self, kind: EventKind) -> bool {
        match self {
            Self::One(k) => *k == kind,
            Self::Many { r#in } => r#in.contains(&kind),
        }
    }
}

#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Filter {
    #[serde(
        rename = "type",
        alias = "event",
        skip_serializing_if = "Option::is_none"
    )]
    kind: Option<Kinds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    side: Option<Seat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    formation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<ObjectRef>,
}
impl Filter {
    fn needs_identity(&self) -> bool {
        self.side.is_some() || self.object.is_some() || self.formation.is_some()
    }

    fn matches(&self, row: &Row<'_>) -> bool {
        self.kind
            .as_ref()
            .is_none_or(|k| k.matches(row.event.payload.kind()))
            && self.side.is_none_or(|s| row.side == Some(s))
            && self.formation.is_none_or(|f| row.formation == Some(f))
            && self.object.is_none_or(|o| row.object == Some(o))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Until {
    FightEnd,
    Tick(u32),
    Event(EventKind),
    Any(Vec<Self>),
    All(Vec<Self>),
    Count {
        #[serde(rename = "where")]
        filter: Filter,
        at_least: u64,
        #[serde(skip)]
        seen: u64,
    },
    Metric {
        field: Metric,
        at_least: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        side: Option<Seat>,
        #[serde(skip_serializing_if = "Option::is_none")]
        formation: Option<u64>,
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Metric {
    DamageTaken,
    DamageReal,
    Kills,
    AliveMembers,
}

impl Until {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Any(xs) | Self::All(xs) if xs.is_empty() => {
                Err(Error::new("any/all needs a condition"))
            }
            Self::Any(xs) | Self::All(xs) => xs.iter().try_for_each(Self::validate),
            _ => Ok(()),
        }
    }
    fn event(&mut self, row: &Row<'_>) {
        match self {
            Self::Count { filter, seen, .. } if filter.matches(row) => *seen += 1,
            Self::All(xs) | Self::Any(xs) => xs.iter_mut().for_each(|x| x.event(row)),
            _ => {}
        }
    }
    fn needs_events(&self) -> bool {
        match self {
            Self::Count { .. } => true,
            Self::Any(xs) | Self::All(xs) => xs.iter().any(Self::needs_events),
            _ => false,
        }
    }
    fn needs_identity(&self) -> bool {
        match self {
            Self::Count { filter, .. } => filter.needs_identity(),
            Self::Metric { formation, .. } => formation.is_some(),
            Self::Any(xs) | Self::All(xs) => xs.iter().any(Self::needs_identity),
            _ => false,
        }
    }
    fn matches(
        &self,
        tick: u32,
        events: &[Event],
        sim: &Simulation,
        numbering: &UnitNumbering,
    ) -> bool {
        match self {
            Self::FightEnd => false,
            Self::Tick(n) => tick >= *n,
            Self::Event(kind) => events.iter().any(|e| e.payload.kind() == *kind),
            Self::Any(xs) => xs.iter().any(|x| x.matches(tick, events, sim, numbering)),
            Self::All(xs) => xs.iter().all(|x| x.matches(tick, events, sim, numbering)),
            Self::Count { at_least, seen, .. } => seen >= at_least,
            Self::Metric {
                field,
                at_least,
                side,
                formation,
            } => {
                metric_values(*field, sim, numbering, *side, *formation)
                    .map(|(_, _, n)| n)
                    .sum::<i64>()
                    >= *at_least
            }
        }
    }
    fn reason(&self, tick: u32, events: &[Event], sim: &Simulation, n: &UnitNumbering) -> Value {
        if let Self::Any(xs) = self
            && let Some(x) = xs.iter().find(|x| x.matches(tick, events, sim, n))
        {
            return x.reason(tick, events, sim, n);
        }
        json!(self)
    }
}

#[derive(Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Field {
    Tick,
    Ordinal,
    Type,
    Object,
    Position,
    Side,
    Formation,
    Source,
    Target,
    LastHit,
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Group {
    Side,
    Formation,
    Object,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Preset {
    Preview,
    Settlement,
    CoreDamage,
    OpeningDeaths,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Table {
    Events,
}
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Collect {
    Preset(Preset),
    Rows {
        #[serde(rename = "from")]
        _from: Table,
        #[serde(default, rename = "where")]
        filter: Filter,
        select: Vec<Field>,
    },
    Count {
        count: Filter,
        group_by: Option<Group>,
    },
    First {
        first: Filter,
        group_by: Option<Group>,
    },
    Last {
        last: Filter,
        group_by: Option<Group>,
    },
    Nth {
        nth: usize,
        #[serde(rename = "where")]
        filter: Filter,
        group_by: Option<Group>,
    },
    AtStop {
        at_stop: Metric,
        group_by: Option<Group>,
    },
}

#[derive(Clone, Copy)]
struct Row<'a> {
    tick: u32,
    ordinal: usize,
    event: &'a Event,
    object: Option<ObjectRef>,
    side: Option<Seat>,
    formation: Option<u64>,
    numbering: &'a UnitNumbering,
    last_hit: Option<(Option<ObjectRef>, i32)>,
}
fn canonical(n: &UnitNumbering, mut r: ObjectRef) -> ObjectRef {
    if r.kind == ObjectKind::Unit {
        r.id = n.unit(r.id);
    }
    r
}
impl Row<'_> {
    fn project(&self, fields: &[Field]) -> Value {
        let mut map = serde_json::Map::new();
        for field in fields {
            let (key, value) = match field {
                Field::Tick => ("tick", json!(self.tick)),
                Field::Ordinal => ("ordinal", json!(self.ordinal)),
                Field::Type => ("type", json!(self.event.kind())),
                Field::Object => ("object", json!(self.object)),
                Field::Side => ("side", json!(self.side)),
                Field::Formation => ("formation", json!(self.formation)),
                Field::Source => (
                    "source",
                    json!(self.event.source.map(|r| canonical(self.numbering, r))),
                ),
                Field::Target => (
                    "target",
                    json!(self.event.target.map(|r| canonical(self.numbering, r))),
                ),
                Field::LastHit => (
                    "last_hit",
                    json!(
                        self.last_hit
                            .map(|(source, amount)| json!({"source":source,"amount":amount}))
                    ),
                ),
                Field::Position => (
                    "position",
                    match &self.event.payload {
                        EventPayload::UnitDied { position }
                        | EventPayload::BuildingDestroyed { position }
                        | EventPayload::ShieldDestroyed { position, .. }
                        | EventPayload::ShieldCreated { position, .. }
                        | EventPayload::UnitCreated { position, .. }
                        | EventPayload::ProjectileRemoved { position, .. }
                        | EventPayload::TerrainCreated { position, .. }
                        | EventPayload::TerrainRemoved { position, .. }
                        | EventPayload::TerrainConverted { position } => json!(position),
                        _ => Value::Null,
                    },
                ),
            };
            map.insert(key.into(), value);
        }
        Value::Object(map)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Total,
    Side(Seat),
    Formation(u64),
    Object(ObjectRef),
    Unknown,
}
impl Key {
    fn of(
        group: Option<Group>,
        side: Option<Seat>,
        formation: Option<u64>,
        object: Option<ObjectRef>,
    ) -> Self {
        match group {
            None => Self::Total,
            Some(Group::Side) => side.map_or(Self::Unknown, Self::Side),
            Some(Group::Formation) => formation.map_or(Self::Unknown, Self::Formation),
            Some(Group::Object) => object.map_or(Self::Unknown, Self::Object),
        }
    }
    fn value(self) -> Value {
        match self {
            Self::Total | Self::Unknown => Value::Null,
            Self::Side(s) => json!(s),
            Self::Formation(f) => json!(f),
            Self::Object(o) => json!(o),
        }
    }
}

struct Aggregate {
    spec: Collect,
    rows: Vec<Value>,
    groups: BTreeMap<Key, (usize, Option<u32>)>,
}
impl Aggregate {
    fn needs_identity(&self) -> bool {
        match &self.spec {
            Collect::Rows { filter, select, .. } => {
                filter.needs_identity()
                    || select.iter().any(|f| {
                        !matches!(
                            f,
                            Field::Tick | Field::Ordinal | Field::Position | Field::Type
                        )
                    })
            }
            Collect::Count {
                count: filter,
                group_by,
            }
            | Collect::First {
                first: filter,
                group_by,
            }
            | Collect::Last {
                last: filter,
                group_by,
            }
            | Collect::Nth {
                filter, group_by, ..
            } => filter.needs_identity() || group_by.is_some(),
            Collect::AtStop { group_by, .. } => matches!(group_by, Some(Group::Formation)),
            Collect::Preset(p) => matches!(p, Preset::Settlement | Preset::OpeningDeaths),
        }
    }
    fn observe(&mut self, row: &Row<'_>) {
        let (filter, group) = match &self.spec {
            Collect::Rows { filter, select, .. } => {
                if filter.matches(row) {
                    self.rows.push(row.project(select));
                }
                return;
            }
            Collect::Count { count, group_by } => (count, *group_by),
            Collect::First { first, group_by } => (first, *group_by),
            Collect::Last { last, group_by } => (last, *group_by),
            Collect::Nth {
                filter, group_by, ..
            } => (filter, *group_by),
            _ => return,
        };
        if !filter.matches(row) {
            return;
        }
        let entry = self
            .groups
            .entry(Key::of(group, row.side, row.formation, row.object))
            .or_default();
        entry.0 += 1;
        if matches!(self.spec, Collect::Last { .. })
            || matches!(self.spec, Collect::First { .. }) && entry.0 == 1
            || matches!(self.spec,Collect::Nth{nth,..} if nth==entry.0)
        {
            entry.1 = Some(row.tick);
        }
    }

    fn finish(self, sim: &Simulation, n: &UnitNumbering) -> Option<Value> {
        let group = match self.spec {
            Collect::Rows { .. } => return Some(json!(self.rows)),
            Collect::AtStop { at_stop, group_by } => {
                let mut map = BTreeMap::<Key, i64>::new();
                for (s, f, value) in metric_values(at_stop, sim, n, None, None) {
                    *map.entry(Key::of(group_by, Some(s), Some(f), None))
                        .or_default() += value;
                }
                return Some(if group_by.is_none() {
                    json!(map.values().sum::<i64>())
                } else {
                    json!(
                        map.into_iter()
                            .map(|(k, v)| json!({"key":k.value(),"value":v}))
                            .collect::<Vec<_>>()
                    )
                });
            }
            Collect::Count { group_by, .. }
            | Collect::First { group_by, .. }
            | Collect::Last { group_by, .. }
            | Collect::Nth { group_by, .. } => group_by,
            Collect::Preset(_) => return None,
        };
        let value = |(count, tick): (usize, Option<u32>)| {
            if matches!(self.spec, Collect::Count { .. }) {
                json!(count)
            } else {
                json!(tick)
            }
        };
        Some(if group.is_none() {
            value(self.groups.get(&Key::Total).copied().unwrap_or_default())
        } else {
            json!(
                self.groups
                    .into_iter()
                    .map(|(k, v)| json!({"key":k.value(),"value":value(v)}))
                    .collect::<Vec<_>>()
            )
        })
    }
}

fn metric_values(
    field: Metric,
    sim: &Simulation,
    n: &UnitNumbering,
    side: Option<Seat>,
    formation: Option<u64>,
) -> impl Iterator<Item = (Seat, u64, i64)> {
    let alive = matches!(field, Metric::AliveMembers);
    let actors = alive
        .then_some(&sim.actors)
        .into_iter()
        .flat_map(|actors| actors.values())
        .filter(|a| a.alive())
        .map(|a| {
            (
                Seat::of(a.placement.team),
                n.formation(a.placement.formation_id),
                1,
            )
        });
    let statistics = (!alive)
        .then_some(&sim.statistics.recorders)
        .into_iter()
        .flat_map(|recorders| recorders.values())
        .filter(|s| s.recorder == RecorderKind::Formation)
        .map(move |s| {
            let v = match field {
                Metric::DamageTaken => s.damage_taken,
                Metric::DamageReal => s.damage_real,
                Metric::Kills => s.kills,
                Metric::AliveMembers => 0,
            };
            (
                Seat::of(s.team_id),
                n.formation(s.recorder_id),
                i64::from(v),
            )
        });
    actors.chain(statistics).filter(move |(s, f, _)| {
        side.is_none_or(|seat| seat == *s) && formation.is_none_or(|id| id == *f)
    })
}

/// Compiled once per request. A missing projection requests the complete settlement.
pub struct ObservationPlan {
    until: Until,
    collectors: BTreeMap<String, Aggregate>,
}

impl ObservationPlan {
    /// # Errors
    /// Unknown fields, metrics and invalid predicates are rejected before running.
    pub fn parse(
        until: Option<Value>,
        collect: Option<Value>,
    ) -> std::result::Result<Self, String> {
        let until: Until = serde_json::from_value(until.unwrap_or_else(|| json!("fight_end")))
            .map_err(|e| e.to_string())?;
        until.validate().map_err(|e| e.to_string())?;
        let fields: BTreeMap<String, Collect> =
            serde_json::from_value(collect.unwrap_or_else(|| json!({"settlement":"settlement"})))
                .map_err(|e| e.to_string())?;
        let mut collectors = BTreeMap::new();
        for (name, spec) in fields {
            if matches!(
                spec,
                Collect::Nth { nth: 0, .. }
                    | Collect::AtStop {
                        group_by: Some(Group::Object),
                        ..
                    }
            ) {
                return Err(format!("invalid nth or metric grouping for {name}"));
            }
            collectors.insert(
                name,
                Aggregate {
                    spec,
                    rows: Vec::new(),
                    groups: BTreeMap::new(),
                },
            );
        }
        Ok(Self { until, collectors })
    }

    #[must_use]
    pub fn settlement_names(&self) -> Vec<String> {
        self.collectors
            .iter()
            .filter(|(_, a)| matches!(a.spec, Collect::Preset(Preset::Settlement)))
            .map(|(s, _)| s.clone())
            .collect()
    }

    fn previews(
        &self,
        compiled: &CompiledLayout,
        layout: &mechcore_document::Layout,
        config: &SimulationConfig,
        seed: i32,
    ) -> Result<BTreeMap<String, Value>> {
        let mut result = BTreeMap::new();
        let names = self.collectors.iter().filter_map(|(name, collector)| {
            matches!(collector.spec, Collect::Preset(Preset::Preview)).then_some(name)
        });
        let mut names = names.peekable();
        if names.peek().is_some() {
            let preview = json!(super::preview(compiled, layout, config, seed, "layout")?);
            for name in names {
                result.insert(name.clone(), preview.clone());
            }
        }
        Ok(result)
    }

    /// Runs the plan on the supplied deployment in memory.
    /// # Errors
    /// Reports compilation, simulation, observation or cancellation failures.
    pub fn run(
        mut self,
        layout: &mechcore_document::Layout,
        cancel: &AtomicBool,
    ) -> Result<Observation> {
        static CONFIG: LazyLock<Result<SimulationConfig>> = LazyLock::new(SimulationConfig::load);
        let config = CONFIG.as_ref().map_err(|e| Error::new(e.to_string()))?;
        let zero = matches!(self.until, Until::Tick(0));
        let (_, compiled) =
            crate::layout::compile_deployment(layout.clone().normalized(), &config.units, zero)?;
        let seed = layout
            .seed
            .ok_or_else(|| Error::new("arena simulation requires the match seed"))?;
        let mut values = self.previews(&compiled, layout, config, seed)?;
        if zero && values.len() == self.collectors.len() && !values.is_empty() {
            return Ok(Observation {
                tick: 0,
                settled: false,
                stop: json!(self.until),
                values,
                evidence: None,
            });
        }
        let mut sim = Simulation::new_unprepared(
            &compiled,
            &config.units,
            &config.towers,
            &config.maps,
            seed,
        )?;
        sim.prepare_and_enter_fight()?;
        let query_events = self.until.needs_events()
            || self.collectors.values().any(|c| {
                !matches!(
                    c.spec,
                    Collect::Preset(Preset::Preview | Preset::CoreDamage | Preset::Settlement)
                        | Collect::AtStop { .. }
                )
            });
        let mut observer = BattleObserver::new(&self, &sim, query_events);
        let mut reason = self.until.reason(0, &[], &sim, &observer.numbering);
        let (tick, settled) = if self.until.matches(0, &[], &sim, &observer.numbering) {
            (0, false)
        } else {
            let (tick, end) = advance_fight(&mut sim, |sim, tick, events, _| {
                if cancel.load(AtomicOrdering::Relaxed) {
                    return Err(Error::new("arena trial cancelled"));
                }
                let events = &events.events;
                let terminal = sim.ready_to_finish() || u64::from(tick) == fight_ticks();
                observer.snapshots(sim, tick, events, terminal);
                if query_events {
                    observer.events(&mut self, tick, events);
                }
                if !terminal && self.until.matches(tick, events, sim, &observer.numbering) {
                    reason = self.until.reason(tick, events, sim, &observer.numbering);
                    return Ok(true);
                }
                Ok(false)
            })?;
            (tick, end != "observer")
        };
        if settled {
            reason = json!("fight_end");
        }
        for (name, c) in self.collectors {
            match &c.spec {
                Collect::Preset(Preset::CoreDamage) if settled => {
                    let [b, r] = sim.ending.scores;
                    let (blue, red) =
                        mechcore_document::reactor_damage::damage(i64::from(b), i64::from(r));
                    values.insert(name, json!({"blue":blue,"red":red}));
                }
                Collect::Preset(Preset::OpeningDeaths) => {
                    let units = observer.opening_deaths.iter().map(|(id, tick)| {
                        json!({"object": ObjectRef::new(ObjectKind::Unit, observer.numbering.unit(*id)),
                               "first_death_tick": tick})
                    }).collect::<Vec<_>>();
                    values.insert(name, json!(units));
                }
                _ => {
                    if let Some(v) = c.finish(&sim, &observer.numbering) {
                        values.insert(name, v);
                    }
                }
            }
        }
        let evidence = observer.evidence(&sim, settled)?;
        Ok(Observation {
            tick,
            settled,
            stop: reason,
            values,
            evidence,
        })
    }
}

#[derive(Default)]
struct BattleObserver {
    numbering: UnitNumbering,
    identities: BTreeMap<ObjectRef, (Option<Seat>, Option<u64>)>,
    opening_deaths: BTreeMap<u64, Option<u32>>,
    first: Option<WorldSnapshot>,
    last: Option<WorldSnapshot>,
    shields: BTreeMap<u64, ShieldState>,
    identity: bool,
    last_hits: bool,
    full: bool,
}

impl BattleObserver {
    fn new(plan: &ObservationPlan, sim: &Simulation, query_events: bool) -> Self {
        let full = !plan.settlement_names().is_empty();
        let cohort = plan
            .collectors
            .values()
            .any(|a| matches!(a.spec, Collect::Preset(Preset::OpeningDeaths)));
        let last_hits = plan.collectors.values().any(
            |a| matches!(&a.spec,Collect::Rows{select,..} if select.contains(&Field::LastHit)),
        );
        let identity =
            plan.until.needs_identity() || plan.collectors.values().any(Aggregate::needs_identity);
        let mut this = Self {
            identity,
            last_hits,
            full,
            ..Self::default()
        };
        if identity && query_events {
            for (&id, a) in &sim.actors {
                let key = ObjectRef::new(ObjectKind::Unit, id);
                this.identities.insert(
                    key,
                    (
                        Some(Seat::of(a.placement.team)),
                        Some(a.placement.formation_id),
                    ),
                );
                if cohort {
                    this.opening_deaths.insert(id, None);
                }
            }
            for b in &sim.buildings {
                this.identities.insert(
                    ObjectRef::new(ObjectKind::Building, b.building_id),
                    (Some(Seat::of(b.team_id)), None),
                );
            }
            for s in sim.shield_states() {
                this.identities.insert(
                    ObjectRef::new(ObjectKind::Shield, s.shield_id),
                    (Some(Seat::of(s.team_id)), None),
                );
            }
        }
        this
    }

    fn snapshots(&mut self, sim: &Simulation, tick: u32, events: &[Event], terminal: bool) {
        // Only new identities require a snapshot for canonical MCFR numbering.
        if self.identity
            && (tick == 1
                || events
                    .iter()
                    .any(|e| matches!(e.payload, EventPayload::UnitCreated { .. }))
                || sim
                    .actors
                    .iter()
                    .any(|(id, a)| a.alive() && !self.numbering.has_unit(*id)))
        {
            let mut state = sim.snapshot();
            if tick == 1 {
                self.numbering = UnitNumbering::of_first_snapshot(&state, events);
            } else {
                self.numbering.number_new_units(&state, events);
            }
            if self.full && tick == 1 {
                self.numbering.apply(&mut state, &mut []);
                self.first = Some(state);
            }
        }
        if self.full
            && (tick == 1
                || events
                    .iter()
                    .any(|e| matches!(e.payload, EventPayload::ShieldCreated { .. })))
        {
            for mut shield in sim.shield_states() {
                shield.owner = shield.owner.map(|r| canonical(&self.numbering, r));
                self.shields.entry(shield.shield_id).or_insert(shield);
            }
        }
        if self.full && terminal {
            let mut state = sim.snapshot();
            self.numbering.apply(&mut state, &mut []);
            self.last = Some(state);
        }
    }
    fn events(&mut self, plan: &mut ObservationPlan, tick: u32, events: &[Event]) {
        let mut hits = BTreeMap::new();
        for (ordinal, e) in events.iter().enumerate() {
            if self.identity
                && let Some(subject) = e.subject
            {
                match e.payload {
                    EventPayload::UnitCreated {
                        team_id,
                        formation_id,
                        ..
                    } => {
                        self.identities
                            .insert(subject, (Some(Seat::of(team_id)), Some(formation_id)));
                    }
                    EventPayload::UnitTeamChanged { new_team_id, .. } => {
                        self.identities.entry(subject).or_default().0 = Some(Seat::of(new_team_id));
                    }
                    EventPayload::TerrainCreated { team_id, .. } => {
                        self.identities
                            .insert(subject, (team_id.map(Seat::of), None));
                    }
                    EventPayload::ShieldCreated { team_id, .. } => {
                        self.identities
                            .insert(subject, (Some(Seat::of(team_id)), None));
                    }
                    EventPayload::UnitDied { .. } => {
                        if let Some(death) = self.opening_deaths.get_mut(&subject.id) {
                            death.get_or_insert(tick);
                        }
                    }
                    _ => {}
                }
            }
            let (side, formation) = e
                .subject
                .and_then(|o| self.identities.get(&o))
                .copied()
                .unwrap_or_default();
            let row = Row {
                tick,
                ordinal,
                event: e,
                object: e.subject.map(|o| canonical(&self.numbering, o)),
                side,
                formation: formation.map(|f| self.numbering.formation(f)),
                numbering: &self.numbering,
                last_hit: e.subject.and_then(|o| hits.get(&o)).copied(),
            };
            plan.until.event(&row);
            for c in plan.collectors.values_mut() {
                c.observe(&row);
            }
            if self.last_hits
                && let EventPayload::Damage { amount, .. } = e.payload
                && let Some(target) = e.target
            {
                hits.insert(
                    target,
                    (e.source.map(|o| canonical(&self.numbering, o)), amount),
                );
            }
        }
    }

    fn evidence(self, sim: &Simulation, settled: bool) -> Result<Option<SettlementEvidence>> {
        Ok(if self.full && settled {
            let mut settlement = sim.settlement()?;
            self.numbering.apply_settlement(&mut settlement);
            Some(SettlementEvidence {
                first: self.first.ok_or_else(|| Error::new("missing first tick"))?,
                last: self
                    .last
                    .ok_or_else(|| Error::new("missing terminal tick"))?,
                shields: self.shields,
                settlement,
            })
        } else {
            None
        })
    }
}
