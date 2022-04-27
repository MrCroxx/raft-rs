// Copyright 2019 TiKV Project Authors. Licensed under Apache-2.0.

// We use `default` method a lot to be support prost and rust-protobuf at the
// same time. And reassignment can be optimized by compiler.
#![allow(clippy::field_reassign_with_default)]

use criterion::{black_box, Bencher, BenchmarkId, Criterion, Throughput};
use raft::eraftpb::{ConfState, Entry, Message, Snapshot, SnapshotMetadata};
use raft::{storage::MemStorage, Config, RawNode};

use std::time::{Duration, Instant};

pub fn bench_raw_node(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    bench_raw_node_new(c, rt);
    bench_raw_node_leader_propose(c, rt);
    bench_raw_node_new_ready(c, rt);
}

async fn quick_raw_node(logger: &slog::Logger) -> RawNode<MemStorage> {
    let id = 1;
    let conf_state = ConfState::from((vec![1], vec![]));
    let storage = MemStorage::new_with_conf_state(conf_state).await;
    let config = Config::new(id);
    RawNode::new(&config, storage, logger).await.unwrap()
}

pub fn bench_raw_node_new(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    let logger = raft::default_logger();
    c.bench_with_input(BenchmarkId::new("RawNode::new", ""), &logger, |b, l| {
        b.to_async(rt).iter(|| async { quick_raw_node(l).await })
    });
}

pub fn bench_raw_node_leader_propose(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    static KB: usize = 1024;
    let mut test_sets = vec![
        0,
        32,
        128,
        512,
        KB,
        4 * KB,
        16 * KB,
        128 * KB,
        512 * KB,
        KB * KB,
    ];
    let mut group = c.benchmark_group("RawNode::leader_propose");
    for size in test_sets.drain(..) {
        // Calculate measurement time in seconds according to the input size.
        // The approximate time might not be the best but should work fine.
        let mtime = if size < KB {
            1
        } else if size < 128 * KB {
            3
        } else {
            7
        };

        group
            .measurement_time(Duration::from_secs(mtime))
            .throughput(Throughput::Bytes(size as u64))
            .bench_with_input(
                BenchmarkId::new("RawNode::propose", size),
                &size,
                |b: &mut Bencher, size| {
                    b.to_async(rt).iter_custom(|iter| async move {
                        let logger = raft::default_logger();
                        let mut node = quick_raw_node(&logger).await;
                        node.raft.become_candidate().await;
                        node.raft.become_leader().await;
                        let dataset = vec![(vec![0; 8], vec![0; *size]); iter as usize];
                        let start = Instant::now();
                        for (context, value) in dataset {
                            black_box(node.propose(context, value).await).unwrap();
                        }
                        start.elapsed()
                    });
                },
            );
    }
}

pub fn bench_raw_node_new_ready(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    let mut group = c.benchmark_group("RawNode::ready");
    group
        // TODO: The proper measurement time could be affected by the system and machine.
        .measurement_time(Duration::from_secs(20))
        .bench_function("Default", |b: &mut Bencher| {
            b.to_async(rt).iter_custom(|iters| async move {
                let logger = raft::default_logger();
                let mut nodes = Vec::with_capacity(iters as usize);
                for _ in 0..iters {
                    let node = test_ready_raft_node(&logger).await;
                    nodes.push(node);
                }
                let start = Instant::now();
                for mut node in nodes {
                    black_box(node.ready().await);
                }
                start.elapsed()
            })
        });
}

// Create a raft node calling `ready()` with things below:
//  - 100 new entries with 32KB data each
//  - 100 committed entries with 32KB data each
//  - 100 raft messages
//  - A snapshot with 8MB data
// TODO: Maybe gathering all the things we need into a struct(e.g. something like `ReadyBenchOption`) and use it
//       to customize the output.
async fn test_ready_raft_node(logger: &slog::Logger) -> RawNode<MemStorage> {
    let mut node = quick_raw_node(logger).await;
    node.raft.become_candidate().await;
    node.raft.become_leader().await;
    let unstable = node.raft.raft_log.unstable_entries().to_vec();
    node.raft.raft_log.stable_entries(1, 1);
    node.raft.raft_log.store.wl().append(&unstable).expect("");
    node.raft.on_persist_entries(1, 1).await;
    node.raft.commit_apply(1).await;
    let mut entries = vec![];
    for i in 1..101 {
        let mut e = Entry::default();
        e.data = vec![0; 32 * 1024];
        e.context = vec![];
        e.index = i;
        e.term = 1;
        entries.push(e);
    }
    let _ = node.raft.append_entry(&mut entries).await;
    let unstable = node.raft.raft_log.unstable_entries().to_vec();
    node.raft.raft_log.stable_entries(101, 1);
    node.raft.raft_log.store.wl().append(&unstable).expect("");
    // This increases 'committed_index' to `last_index` because there is only one node in quorum.
    node.raft.on_persist_entries(101, 1).await;

    let mut snap = Snapshot::default();
    snap.data = vec![0; 8 * 1024 * 1024];
    // We don't care about the contents in snapshot here since it won't be applied.
    snap.metadata = Some(SnapshotMetadata {
        conf_state: Some(ConfState::default()),
        index: 0,
        term: 0,
    });
    for _ in 0..100 {
        node.raft.msgs.push(Message::default());
    }
    // Force reverting committed index to provide us some entries to be stored from next `Ready`
    node.raft.raft_log.committed = 1;
    node
}
