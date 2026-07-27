use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct State {
    queue: VecDeque<(usize, usize)>,
    next_by_producer: [usize; 2],
    received: Vec<(usize, usize)>,
}

fn explore(state: State, visited: &mut BTreeSet<State>, terminal: &mut usize) {
    if !visited.insert(state.clone()) {
        return;
    }
    if state.received.len() == 4 {
        let mut delivered = state.received.clone();
        delivered.sort_unstable();
        assert_eq!(delivered, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
        for producer in 0..2 {
            let sequence = state
                .received
                .iter()
                .filter_map(|(owner, value)| (*owner == producer).then_some(*value))
                .collect::<Vec<_>>();
            assert_eq!(sequence, vec![0, 1], "per-producer FIFO must hold");
        }
        *terminal += 1;
        return;
    }

    let mut enabled = 0usize;
    if state.queue.is_empty() {
        for producer in 0..2 {
            if state.next_by_producer[producer] < 2 {
                enabled += 1;
                let mut next = state.clone();
                let value = next.next_by_producer[producer];
                next.next_by_producer[producer] += 1;
                next.queue.push_back((producer, value));
                explore(next, visited, terminal);
            }
        }
    }
    if !state.queue.is_empty() {
        for _consumer in 0..2 {
            enabled += 1;
            let mut next = state.clone();
            let value = next.queue.pop_front().expect("queue is non-empty");
            next.received.push(value);
            explore(next, visited, terminal);
        }
    }
    assert!(enabled > 0, "bounded queue model must not deadlock");
}

#[test]
fn bounded_queue_scheduler_model_has_no_loss_duplication_or_deadlock() {
    let initial = State {
        queue: VecDeque::new(),
        next_by_producer: [0, 0],
        received: Vec::new(),
    };
    let mut visited = BTreeSet::new();
    let mut terminal = 0usize;
    explore(initial, &mut visited, &mut terminal);
    assert!(terminal > 0, "model should reach terminal schedules");
    assert!(
        visited.len() >= 20,
        "model should explore meaningful interleavings"
    );
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WaitSource {
    ReadFd,
    WriteFd,
    Timer,
}

impl WaitSource {
    const ALL: [Self; 3] = [Self::ReadFd, Self::WriteFd, Self::Timer];

    const fn index(self) -> usize {
        match self {
            Self::ReadFd => 0,
            Self::WriteFd => 1,
            Self::Timer => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Subscription {
    epoch: u64,
    source: WaitSource,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Notification {
    epoch: u64,
    source: WaitSource,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WaitPhase {
    InitialCheck,
    Registering(usize),
    PostRegistrationCheck,
    Parked,
    Rearming(WaitSource),
    Resolved,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WaitState {
    epoch: u64,
    phase: WaitPhase,
    ready: [bool; 3],
    active: BTreeSet<Subscription>,
    cleaned: BTreeSet<Subscription>,
    notifications: VecDeque<Notification>,
    winner: Option<WaitSource>,
    resolutions: usize,
    rearms: usize,
}

impl WaitState {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            phase: WaitPhase::InitialCheck,
            ready: [false; 3],
            active: BTreeSet::new(),
            cleaned: BTreeSet::new(),
            notifications: VecDeque::new(),
            winner: None,
            resolutions: 0,
            rearms: 0,
        }
    }

    fn subscription(&self, source: WaitSource) -> Subscription {
        Subscription {
            epoch: self.epoch,
            source,
        }
    }

    fn mark_ready(&mut self, source: WaitSource) {
        self.ready[source.index()] = true;
        let subscription = self.subscription(source);
        if self.active.contains(&subscription) {
            self.notifications.push_back(Notification {
                epoch: self.epoch,
                source,
            });
        }
    }

    fn consume_readiness(&mut self, source: WaitSource) {
        self.ready[source.index()] = false;
    }

    fn inject_notification(&mut self, notification: Notification) {
        self.notifications.push_back(notification);
    }

    fn first_ready(&self) -> Option<WaitSource> {
        WaitSource::ALL
            .into_iter()
            .find(|source| self.ready[source.index()])
    }

    fn resolve(&mut self, source: WaitSource) {
        if self.winner.is_some() {
            return;
        }
        self.winner = Some(source);
        self.resolutions += 1;
        self.ready[source.index()] = false;
        self.cleaned.append(&mut self.active);
        self.notifications.clear();
        self.phase = WaitPhase::Resolved;
    }

    fn advance_protocol(&mut self) -> bool {
        match self.phase {
            WaitPhase::InitialCheck => {
                if let Some(source) = self.first_ready() {
                    self.resolve(source);
                } else {
                    self.phase = WaitPhase::Registering(0);
                }
                true
            }
            WaitPhase::Registering(index) => {
                let source = WaitSource::ALL[index];
                self.active.insert(self.subscription(source));
                self.phase = if index + 1 == WaitSource::ALL.len() {
                    WaitPhase::PostRegistrationCheck
                } else {
                    WaitPhase::Registering(index + 1)
                };
                true
            }
            WaitPhase::PostRegistrationCheck => {
                if let Some(source) = self.first_ready() {
                    self.resolve(source);
                } else {
                    self.phase = WaitPhase::Parked;
                }
                true
            }
            WaitPhase::Parked => {
                let Some(notification) = self.notifications.pop_front() else {
                    return false;
                };
                if notification.epoch != self.epoch
                    || !self.active.contains(&Subscription {
                        epoch: notification.epoch,
                        source: notification.source,
                    })
                {
                    return true;
                }
                if self.ready[notification.source.index()] {
                    self.resolve(notification.source);
                } else {
                    self.active.remove(&self.subscription(notification.source));
                    self.phase = WaitPhase::Rearming(notification.source);
                }
                true
            }
            WaitPhase::Rearming(source) => {
                self.rearms += 1;
                self.active.insert(self.subscription(source));
                if self.ready[source.index()] {
                    self.resolve(source);
                } else {
                    self.phase = WaitPhase::Parked;
                }
                true
            }
            WaitPhase::Resolved => false,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ArrivalInterleaving {
    wait: WaitState,
    readiness_arrived: bool,
}

fn explore_readiness_arrival(
    state: ArrivalInterleaving,
    visited: &mut BTreeSet<ArrivalInterleaving>,
    arrival_phases: &mut BTreeSet<WaitPhase>,
    terminals: &mut usize,
) {
    if !visited.insert(state.clone()) {
        return;
    }
    if state.wait.phase == WaitPhase::Resolved {
        assert!(state.readiness_arrived);
        assert_eq!(state.wait.winner, Some(WaitSource::ReadFd));
        assert_eq!(state.wait.resolutions, 1);
        assert!(state.wait.active.is_empty());
        *terminals += 1;
        return;
    }

    let mut enabled = 0;
    if !state.readiness_arrived {
        enabled += 1;
        arrival_phases.insert(state.wait.phase);
        let mut next = state.clone();
        next.readiness_arrived = true;
        next.wait.mark_ready(WaitSource::ReadFd);
        explore_readiness_arrival(next, visited, arrival_phases, terminals);
    }

    let mut next = state;
    if next.wait.advance_protocol() {
        enabled += 1;
        explore_readiness_arrival(next, visited, arrival_phases, terminals);
    }
    assert!(
        enabled > 0,
        "the wait protocol must not strand an outstanding readiness edge"
    );
}

#[test]
fn wait_registration_model_has_no_lost_wake_in_any_registration_interleaving() {
    let initial = ArrivalInterleaving {
        wait: WaitState::new(1),
        readiness_arrived: false,
    };
    let mut visited = BTreeSet::new();
    let mut arrival_phases = BTreeSet::new();
    let mut terminals = 0;
    explore_readiness_arrival(initial, &mut visited, &mut arrival_phases, &mut terminals);

    assert!(terminals > 0);
    assert!(arrival_phases.contains(&WaitPhase::InitialCheck));
    assert!(arrival_phases.contains(&WaitPhase::Registering(0)));
    assert!(arrival_phases.contains(&WaitPhase::Registering(1)));
    assert!(arrival_phases.contains(&WaitPhase::Registering(2)));
    assert!(arrival_phases.contains(&WaitPhase::PostRegistrationCheck));
    assert!(arrival_phases.contains(&WaitPhase::Parked));
}

fn park_with_all_sources_registered(epoch: u64) -> WaitState {
    let mut wait = WaitState::new(epoch);
    while wait.phase != WaitPhase::Parked {
        assert!(wait.advance_protocol());
    }
    assert_eq!(wait.active.len(), WaitSource::ALL.len());
    wait
}

#[test]
fn multiple_and_duplicate_notifications_choose_one_winner_and_clean_all_tokens() {
    let mut wait = park_with_all_sources_registered(7);
    let expected_tokens = wait.active.clone();

    wait.mark_ready(WaitSource::WriteFd);
    wait.mark_ready(WaitSource::WriteFd);
    wait.mark_ready(WaitSource::Timer);
    assert!(wait.advance_protocol());
    assert_eq!(wait.winner, Some(WaitSource::WriteFd));
    assert_eq!(wait.resolutions, 1);
    assert!(wait.active.is_empty());
    assert_eq!(wait.cleaned, expected_tokens);
    assert!(wait.notifications.is_empty());

    wait.mark_ready(WaitSource::Timer);
    assert!(!wait.advance_protocol());
    assert_eq!(wait.winner, Some(WaitSource::WriteFd));
    assert_eq!(wait.resolutions, 1);
}

#[test]
fn stale_notification_cannot_wake_a_later_wait_epoch() {
    let mut earlier = park_with_all_sources_registered(11);
    earlier.mark_ready(WaitSource::ReadFd);
    let stale = earlier
        .notifications
        .pop_front()
        .expect("ready subscribed source emits a notification");

    let mut later = park_with_all_sources_registered(12);
    later.inject_notification(stale);
    assert!(later.advance_protocol());
    assert_eq!(later.phase, WaitPhase::Parked);
    assert_eq!(later.winner, None);
    assert_eq!(later.resolutions, 0);

    later.mark_ready(WaitSource::ReadFd);
    assert!(later.advance_protocol());
    assert_eq!(later.winner, Some(WaitSource::ReadFd));
    assert_eq!(later.resolutions, 1);
}

#[test]
fn spurious_readiness_rearms_without_spinning_or_stranding_a_racing_edge() {
    let mut wait = park_with_all_sources_registered(19);
    wait.mark_ready(WaitSource::ReadFd);
    wait.consume_readiness(WaitSource::ReadFd);

    assert!(wait.advance_protocol());
    assert_eq!(wait.phase, WaitPhase::Rearming(WaitSource::ReadFd));
    assert_eq!(wait.winner, None);

    wait.mark_ready(WaitSource::ReadFd);
    assert!(wait.advance_protocol());
    assert_eq!(wait.winner, Some(WaitSource::ReadFd));
    assert_eq!(wait.resolutions, 1);
    assert_eq!(wait.rearms, 1);

    let mut quiet_rearm = park_with_all_sources_registered(20);
    quiet_rearm.mark_ready(WaitSource::Timer);
    quiet_rearm.consume_readiness(WaitSource::Timer);
    assert!(quiet_rearm.advance_protocol());
    assert!(quiet_rearm.advance_protocol());
    assert_eq!(quiet_rearm.phase, WaitPhase::Parked);
    assert_eq!(quiet_rearm.rearms, 1);
    assert!(
        !quiet_rearm.advance_protocol(),
        "a consumed readiness edge must not cause a busy loop"
    );

    quiet_rearm.mark_ready(WaitSource::Timer);
    assert!(quiet_rearm.advance_protocol());
    assert_eq!(quiet_rearm.winner, Some(WaitSource::Timer));
}
