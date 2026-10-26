//! Concurrency: a worker pool, a three-stage pipeline, and a fan-out of tasks.
//!
//! The pool runs N small jobs through four workers fed by a bounded queue. The
//! pipeline moves N text records through parse and transform stages, each a task
//! joined to the next by a bounded queue. The fan-out starts N / 50 tasks and
//! joins every result. The work per message is small, so this measures task
//! switching and queue hand-off more than raw computation. Python runs it on one
//! asyncio event loop; Aura and Rust (tokio) use their multi-threaded schedulers.
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

const WORKERS: usize = 4;
const CAPACITY: usize = 64;

fn mix(value: i64) -> i64 {
    let mut x = value;
    for _ in 0..16 {
        x = (x * 31 + 7) % 1000003;
    }
    x
}

async fn produce(jobs: mpsc::Sender<i64>, n: i64) {
    for i in 0..n {
        jobs.send(i).await.unwrap();
    }
}

async fn worker(jobs: Arc<Mutex<mpsc::Receiver<i64>>>, results: mpsc::Sender<i64>) {
    loop {
        let job = jobs.lock().await.recv().await;
        match job {
            Some(job) => results.send(mix(job)).await.unwrap(),
            None => return,
        }
    }
}

async fn source(out: mpsc::Sender<String>, n: i64) {
    for i in 0..n {
        out.send(format!("order-{i}:{}", i * 7919 % 10007))
            .await
            .unwrap();
    }
}

async fn parse(mut lines: mpsc::Receiver<String>, out: mpsc::Sender<i64>) {
    while let Some(line) = lines.recv().await {
        let fields: Vec<&str> = line.split(':').collect();
        let value: i64 = fields[1].parse().unwrap();
        out.send(value + fields[0].len() as i64).await.unwrap();
    }
}

async fn transform(mut values: mpsc::Receiver<i64>, out: mpsc::Sender<i64>) {
    while let Some(value) = values.recv().await {
        out.send(mix(value)).await.unwrap();
    }
}

async fn compute(seed: i64) -> i64 {
    mix(seed * 13)
}

async fn run(n: i64) {
    let (jobs_tx, jobs_rx) = mpsc::channel(CAPACITY);
    let (results_tx, mut results_rx) = mpsc::channel(CAPACITY);
    let jobs_rx = Arc::new(Mutex::new(jobs_rx));
    let mut tasks = vec![tokio::spawn(produce(jobs_tx, n))];
    for _ in 0..WORKERS {
        tasks.push(tokio::spawn(worker(jobs_rx.clone(), results_tx.clone())));
    }
    drop(results_tx);
    let (mut pool_sum, mut pool_max) = (0i64, 0i64);
    for _ in 0..n {
        let value = results_rx.recv().await.unwrap();
        pool_sum += value;
        pool_max = pool_max.max(value);
    }
    for task in tasks {
        task.await.unwrap();
    }

    let (lines_tx, lines_rx) = mpsc::channel(CAPACITY);
    let (values_tx, values_rx) = mpsc::channel(CAPACITY);
    let (mixed_tx, mut mixed_rx) = mpsc::channel(CAPACITY);
    let stages = vec![
        tokio::spawn(source(lines_tx, n)),
        tokio::spawn(parse(lines_rx, values_tx)),
        tokio::spawn(transform(values_rx, mixed_tx)),
    ];
    let (mut items, mut pipeline_sum) = (0i64, 0i64);
    while let Some(value) = mixed_rx.recv().await {
        items += 1;
        pipeline_sum += value;
    }
    for stage in stages {
        stage.await.unwrap();
    }

    let fan_out = n / 50;
    let handles: Vec<_> = (0..fan_out).map(|i| tokio::spawn(compute(i))).collect();
    let mut task_sum = 0;
    for handle in handles {
        task_sum += handle.await.unwrap();
    }
    println!(
        "task_pool jobs={n} pool_sum={pool_sum} pool_max={pool_max} items={items} \
         pipeline_sum={pipeline_sum} tasks={fan_out} task_sum={task_sum}"
    );
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(n));
}
