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
    if state.queue.len() < 1 {
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
