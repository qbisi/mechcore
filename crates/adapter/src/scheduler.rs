//! Who the one game serves next.
//!
//! Every claim is greeted, so the game has many clients and one main thread.
//! A request that needs the game waits here for its turn: the highest level
//! waiting goes first, and clients of one level take turns, so a batch of a
//! thousand rounds and a single round asked for after it share the game
//! rather than one waiting for the other to finish. A started request runs to
//! its end unless a request of a strictly higher level arrives, which stops it
//! at its next polling point; a lease, which holds the game between requests,
//! is taken back the same way.
//!
//! This is the bookkeeping alone. The sockets and the game are the runtime's,
//! which asks [`Scheduler::next`] what to do whenever the game is free.

use mechcore_protocol::{
    Admission, ClientProgress, Operation, QueueSnapshot, Request, RunningRequest, WaitingRequest,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How long a lease may go without a scene request while another client
/// waits. A shell left open at its prompt still polls `status`, which is not
/// use of the scene, so it is what this measures.
pub(crate) const LEASE_IDLE: Duration = Duration::from_secs(60);

pub(crate) type ConnectionId = u64;

struct Connection {
    client: String,
    level: u8,
    /// When this connection last asked for something only the lease allows.
    last_scene_request: Instant,
}

/// A request waiting for its turn.
pub(crate) struct Waiting {
    sequence: u64,
    pub(crate) connection: ConnectionId,
    pub(crate) request: Request,
    since: Instant,
}

struct Running {
    connection: ConnectionId,
    client: String,
    level: u8,
    operation: Operation,
    since: Instant,
}

#[derive(Default)]
struct Tally {
    done: u64,
    failed: u64,
    cancelled: u64,
}

/// What the game should do now that it is free.
pub(crate) enum Next {
    Run(Waiting),
    /// The lease is taken back, for a request of `by_level`.
    Revoke {
        holder: ConnectionId,
        by_level: u8,
    },
    Wait,
}

/// What closing a connection left behind.
pub(crate) struct Closed {
    /// Whether its request was running, and is to be abandoned.
    pub(crate) running: bool,
    /// Whether it held the lease, so the scene it left is nobody's.
    pub(crate) held_lease: bool,
}

#[derive(Default)]
pub(crate) struct Scheduler {
    connections: BTreeMap<ConnectionId, Connection>,
    queue: Vec<Waiting>,
    running: Option<Running>,
    lease: Option<ConnectionId>,
    /// The turn at which each client was last served.
    served: BTreeMap<String, u64>,
    turns: u64,
    sequence: u64,
    tallies: BTreeMap<String, Tally>,
    /// Whether the game has reached its main menu since it started.
    pub(crate) ready: bool,
    /// Since when nobody has been connected and nothing has been asked.
    idle_since: Option<Instant>,
}

impl Scheduler {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            idle_since: Some(now),
            ..Self::default()
        }
    }

    pub(crate) fn connect(&mut self, id: ConnectionId, client: String, level: u8, now: Instant) {
        self.tallies.entry(client.clone()).or_default();
        self.connections.insert(
            id,
            Connection {
                client,
                level,
                last_scene_request: now,
            },
        );
        self.idle_since = None;
    }

    /// Forget a connection and everything it was waiting for.
    pub(crate) fn close(&mut self, id: ConnectionId, now: Instant) -> Closed {
        let Some(connection) = self.connections.remove(&id) else {
            return Closed {
                running: false,
                held_lease: false,
            };
        };
        let before = self.queue.len();
        self.queue.retain(|waiting| waiting.connection != id);
        let dropped = (before - self.queue.len()) as u64;
        self.tallies.entry(connection.client).or_default().cancelled += dropped;
        let held_lease = self.lease == Some(id);
        if held_lease {
            self.lease = None;
        }
        let running = self
            .running
            .as_ref()
            .is_some_and(|running| running.connection == id);
        self.settle_idle(now);
        Closed {
            running,
            held_lease,
        }
    }

    pub(crate) fn holds_lease(&self, id: ConnectionId) -> bool {
        self.lease == Some(id)
    }

    /// Put a request in line, answering where it stands.
    pub(crate) fn enqueue(&mut self, id: ConnectionId, request: Request, now: Instant) -> usize {
        if request.operation.admission() == Admission::Leased
            && let Some(connection) = self.connections.get_mut(&id)
        {
            connection.last_scene_request = now;
        }
        self.sequence += 1;
        let sequence = self.sequence;
        self.queue.push(Waiting {
            sequence,
            connection: id,
            request,
            since: now,
        });
        self.idle_since = None;
        self.order()
            .iter()
            .position(|&index| self.queue[index].sequence == sequence)
            .map_or(self.queue.len(), |position| position + 1)
    }

    /// Whether the running request should be abandoned for the request just
    /// queued by `id`: it is of a strictly higher level. A batch that asks for
    /// the lowest level asks to give way, a watched match of hours included;
    /// requests of one level never stop each other.
    pub(crate) fn outranks_running(&self, id: ConnectionId) -> Option<u8> {
        let running = self.running.as_ref()?;
        if running.connection == id {
            return None;
        }
        let level = self.connections.get(&id)?.level;
        (level > running.level).then_some(level)
    }

    /// Decide what the free game does next, and take it out of the line.
    pub(crate) fn next(&mut self, now: Instant) -> Next {
        if self.running.is_some() {
            return Next::Wait;
        }
        if let Some(holder) = self.lease {
            let Some(connection) = self.connections.get(&holder) else {
                self.lease = None;
                return self.next(now);
            };
            let others = self
                .queue
                .iter()
                .filter(|waiting| waiting.connection != holder)
                .filter_map(|waiting| self.connections.get(&waiting.connection))
                .map(|waiting| waiting.level)
                .max();
            if let Some(level) = others
                && (level > connection.level
                    || now.duration_since(connection.last_scene_request) >= LEASE_IDLE)
            {
                return Next::Revoke {
                    holder,
                    by_level: level,
                };
            }
            return match self
                .queue
                .iter()
                .position(|waiting| waiting.connection == holder)
            {
                Some(index) => self.start(index, now),
                None => Next::Wait,
            };
        }
        if !self.ready {
            return Next::Wait;
        }
        match self.order().first() {
            Some(&index) => self.start(index, now),
            None => Next::Wait,
        }
    }

    fn start(&mut self, index: usize, now: Instant) -> Next {
        let waiting = self.queue.remove(index);
        let Some(connection) = self.connections.get(&waiting.connection) else {
            return Next::Wait;
        };
        self.turns += 1;
        self.served.insert(connection.client.clone(), self.turns);
        if waiting.request.operation == Operation::Lease {
            self.lease = Some(waiting.connection);
        }
        self.running = Some(Running {
            connection: waiting.connection,
            client: connection.client.clone(),
            level: connection.level,
            operation: waiting.request.operation,
            since: now,
        });
        Next::Run(waiting)
    }

    /// The running request ended, well or not.
    pub(crate) fn finish(&mut self, succeeded: bool, now: Instant) {
        if let Some(running) = self.running.take() {
            let tally = self.tallies.entry(running.client).or_default();
            if succeeded {
                tally.done += 1;
            } else {
                tally.failed += 1;
            }
        }
        self.settle_idle(now);
    }

    /// Answer every waiting request as cancelled: the game is stopping.
    pub(crate) fn drain(&mut self) -> Vec<(ConnectionId, u64)> {
        let drained = std::mem::take(&mut self.queue);
        for waiting in &drained {
            if let Some(connection) = self.connections.get(&waiting.connection) {
                self.tallies
                    .entry(connection.client.clone())
                    .or_default()
                    .cancelled += 1;
            }
        }
        drained
            .into_iter()
            .map(|waiting| (waiting.connection, waiting.request.id))
            .collect()
    }

    /// How long nobody has wanted the game.
    pub(crate) fn idle_for(&self, now: Instant) -> Option<Duration> {
        self.idle_since.map(|since| now.duration_since(since))
    }

    fn settle_idle(&mut self, now: Instant) {
        if self.connections.is_empty() && self.queue.is_empty() && self.running.is_none() {
            self.idle_since.get_or_insert(now);
        }
    }

    /// The waiting requests' indices in the order they would be served:
    /// highest level first, then the client served longest ago, then the
    /// earliest asked.
    fn order(&self) -> Vec<usize> {
        let mut served = self.served.clone();
        let mut turns = self.turns;
        let mut left: Vec<usize> = (0..self.queue.len()).collect();
        let mut order = Vec::with_capacity(left.len());
        while !left.is_empty() {
            let key = |index: &usize| {
                let waiting = &self.queue[*index];
                let connection = self.connections.get(&waiting.connection);
                let level = connection.map_or(0, |connection| connection.level);
                let last = connection
                    .and_then(|connection| served.get(&connection.client))
                    .copied()
                    .unwrap_or(0);
                (std::cmp::Reverse(level), last, waiting.sequence)
            };
            let (position, &index) = left
                .iter()
                .enumerate()
                .min_by_key(|(_, index)| key(index))
                .expect("left is not empty");
            left.swap_remove(position);
            if let Some(connection) = self.connections.get(&self.queue[index].connection) {
                turns += 1;
                served.insert(connection.client.clone(), turns);
            }
            order.push(index);
        }
        order
    }

    pub(crate) fn snapshot(&self, now: Instant) -> QueueSnapshot {
        let client = |id: ConnectionId| self.connections.get(&id);
        let queued = self
            .order()
            .into_iter()
            .filter_map(|index| {
                let waiting = &self.queue[index];
                let connection = client(waiting.connection)?;
                Some(WaitingRequest {
                    client: connection.client.clone(),
                    level: connection.level,
                    operation: waiting.request.operation,
                    seconds: now.duration_since(waiting.since).as_secs_f64(),
                })
            })
            .collect();
        let clients = self
            .tallies
            .iter()
            .map(|(name, tally)| {
                let ids: Vec<_> = self
                    .connections
                    .iter()
                    .filter(|(_, connection)| &connection.client == name)
                    .map(|(id, _)| *id)
                    .collect();
                ClientProgress {
                    client: name.clone(),
                    connections: ids.len(),
                    queued: self
                        .queue
                        .iter()
                        .filter(|waiting| ids.contains(&waiting.connection))
                        .count() as u64,
                    done: tally.done,
                    failed: tally.failed,
                    cancelled: tally.cancelled,
                }
            })
            .collect();
        QueueSnapshot {
            ready: self.ready,
            lease: self
                .lease
                .and_then(client)
                .map(|connection| connection.client.clone()),
            running: self.running.as_ref().map(|running| RunningRequest {
                client: running.client.clone(),
                level: running.level,
                operation: running.operation,
                seconds: now.duration_since(running.since).as_secs_f64(),
            }),
            queued,
            clients,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: u64, operation: Operation) -> Request {
        Request {
            id,
            operation,
            arguments: serde_json::json!({}),
        }
    }

    fn ready(now: Instant) -> Scheduler {
        let mut scheduler = Scheduler::new(now);
        scheduler.ready = true;
        scheduler
    }

    fn run(scheduler: &mut Scheduler, now: Instant) -> (ConnectionId, u64) {
        match scheduler.next(now) {
            Next::Run(waiting) => {
                let served = (waiting.connection, waiting.request.id);
                scheduler.finish(true, now);
                served
            }
            Next::Revoke { .. } => panic!("expected a run, got a revocation"),
            Next::Wait => panic!("expected a run, got a wait"),
        }
    }

    #[test]
    fn clients_of_one_level_take_turns() {
        let now = Instant::now();
        let mut scheduler = ready(now);
        scheduler.connect(1, "batch".into(), 1, now);
        scheduler.connect(2, "single".into(), 1, now);
        for id in 1..=3 {
            scheduler.enqueue(1, request(id, Operation::RecordReplayRound), now);
        }
        // Asked for after the whole batch, served second.
        assert_eq!(
            scheduler.enqueue(2, request(9, Operation::RecordReplayRound), now),
            2
        );
        let order: Vec<_> = (0..4).map(|_| run(&mut scheduler, now)).collect();
        assert_eq!(order, [(1, 1), (2, 9), (1, 2), (1, 3)]);
    }

    #[test]
    fn one_client_on_many_connections_is_one_client() {
        let now = Instant::now();
        let mut scheduler = ready(now);
        scheduler.connect(1, "batch".into(), 1, now);
        scheduler.connect(2, "batch".into(), 1, now);
        scheduler.connect(3, "single".into(), 1, now);
        scheduler.enqueue(1, request(1, Operation::RecordReplayRound), now);
        scheduler.enqueue(2, request(1, Operation::RecordReplayRound), now);
        scheduler.enqueue(3, request(1, Operation::RecordReplayRound), now);
        let order: Vec<_> = (0..3).map(|_| run(&mut scheduler, now).0).collect();
        assert_eq!(order, [1, 3, 2]);
    }

    #[test]
    fn a_higher_level_goes_first_and_takes_the_lease_back() {
        let now = Instant::now();
        let mut scheduler = ready(now);
        scheduler.connect(1, "shell".into(), 1, now);
        scheduler.connect(2, "batch".into(), 1, now);
        scheduler.connect(3, "urgent".into(), 3, now);
        scheduler.enqueue(1, request(1, Operation::Lease), now);
        assert_eq!(run(&mut scheduler, now), (1, 1));
        assert!(scheduler.holds_lease(1));
        // An equal level waits for the lease.
        scheduler.enqueue(2, request(1, Operation::RecordReplayRound), now);
        assert!(matches!(scheduler.next(now), Next::Wait));
        // The holder's own requests are served while it holds it.
        scheduler.enqueue(1, request(2, Operation::ApplyLayout), now);
        assert_eq!(run(&mut scheduler, now), (1, 2));
        scheduler.enqueue(3, request(1, Operation::RecordReplayRound), now);
        assert!(matches!(
            scheduler.next(now),
            Next::Revoke {
                holder: 1,
                by_level: 3
            }
        ));
        let closed = scheduler.close(1, now);
        assert!(closed.held_lease && !closed.running);
        assert_eq!(run(&mut scheduler, now), (3, 1));
        assert_eq!(run(&mut scheduler, now), (2, 1));
    }

    #[test]
    fn an_idle_lease_is_taken_back_only_when_someone_waits() {
        let now = Instant::now();
        let later = now + LEASE_IDLE;
        let mut scheduler = ready(now);
        scheduler.connect(1, "shell".into(), 1, now);
        scheduler.connect(2, "batch".into(), 1, now);
        scheduler.enqueue(1, request(1, Operation::Lease), now);
        run(&mut scheduler, now);
        assert!(matches!(scheduler.next(later), Next::Wait));
        scheduler.enqueue(2, request(1, Operation::RecordReplayRound), later);
        assert!(matches!(
            scheduler.next(later),
            Next::Revoke { holder: 1, .. }
        ));
    }

    #[test]
    fn a_running_request_is_outranked_only_by_a_higher_level() {
        let now = Instant::now();
        let mut scheduler = ready(now);
        scheduler.connect(1, "shell".into(), 2, now);
        scheduler.connect(2, "batch".into(), 2, now);
        scheduler.connect(3, "urgent".into(), 3, now);
        scheduler.enqueue(1, request(1, Operation::Lease), now);
        run(&mut scheduler, now);
        scheduler.enqueue(1, request(2, Operation::ApplyLayout), now);
        let Next::Run(_) = scheduler.next(now) else {
            panic!("the holder's request runs");
        };
        assert_eq!(scheduler.outranks_running(2), None);
        assert_eq!(scheduler.outranks_running(3), Some(3));
    }

    #[test]
    fn a_closed_connection_takes_its_waiting_requests_with_it() {
        let now = Instant::now();
        let mut scheduler = ready(now);
        scheduler.connect(1, "batch".into(), 1, now);
        scheduler.enqueue(1, request(1, Operation::RecordReplayRound), now);
        scheduler.enqueue(1, request(2, Operation::RecordReplayRound), now);
        let Next::Run(_) = scheduler.next(now) else {
            panic!("the first request runs");
        };
        let closed = scheduler.close(1, now);
        assert!(closed.running);
        scheduler.finish(false, now);
        let snapshot = scheduler.snapshot(now);
        assert!(snapshot.queued.is_empty());
        assert_eq!(
            snapshot.clients,
            [ClientProgress {
                client: "batch".into(),
                connections: 0,
                queued: 0,
                done: 0,
                failed: 1,
                cancelled: 1,
            }]
        );
        assert_eq!(scheduler.idle_for(now), Some(Duration::ZERO));
    }

    #[test]
    fn nothing_starts_before_the_game_is_ready() {
        let now = Instant::now();
        let mut scheduler = Scheduler::new(now);
        scheduler.connect(1, "batch".into(), 1, now);
        scheduler.enqueue(1, request(1, Operation::RecordReplayRound), now);
        assert!(matches!(scheduler.next(now), Next::Wait));
        scheduler.ready = true;
        assert_eq!(run(&mut scheduler, now), (1, 1));
    }
}
