// Copyright 2019 TiKV Project Authors. Licensed under Apache-2.0.

use crate::DEFAULT_RAFT_SETS;
use criterion::{BenchmarkId, Criterion};
use raft::eraftpb::ConfState;
use raft::{storage::MemStorage, Config, Raft};

pub fn bench_raft(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    bench_raft_new(c, rt);
    bench_raft_campaign(c, rt);
}

async fn new_storage(voters: usize, learners: usize) -> MemStorage {
    let mut cc = ConfState::default();
    for i in 1..=voters {
        cc.voters.push(i as u64);
    }
    for i in 1..=learners {
        cc.learners.push(voters as u64 + i as u64);
    }
    MemStorage::new_with_conf_state(cc).await
}

async fn quick_raft(storage: MemStorage, logger: &slog::Logger) -> Raft<MemStorage> {
    let id = 1;
    let config = Config::new(id);
    Raft::new(&config, storage, logger).await.unwrap()
}

pub fn bench_raft_new(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    for (voters, learners) in DEFAULT_RAFT_SETS.iter() {
        let logger = raft::default_logger();
        let storage = rt.block_on(new_storage(*voters, *learners));
        c.bench_with_input(
            BenchmarkId::new(&format!("Raft::new ({}, {})", voters, learners), ""),
            &(&logger, storage),
            |b, (l, s)| {
                b.to_async(rt)
                    .iter(|| async { quick_raft(s.clone(), l).await })
            },
        );
    }
}

pub fn bench_raft_campaign(c: &mut Criterion, rt: &tokio::runtime::Runtime) {
    for (voters, learners) in DEFAULT_RAFT_SETS.iter().skip(1) {
        let msgs = &[
            "CampaignPreElection",
            "CampaignElection",
            "CampaignTransfer",
        ];
        for msg in msgs {
            let logger = raft::default_logger();
            let storage = rt.block_on(new_storage(*voters, *learners));
            c.bench_with_input(
                BenchmarkId::new(&format!("Raft::new ({}, {})", voters, learners), ""),
                &(&logger, storage, msg),
                |b, (l, s, m)| {
                    b.to_async(rt).iter(|| async {
                        let mut raft = quick_raft(s.clone(), l).await;
                        raft.campaign(m.as_bytes()).await;
                    })
                },
            );
        }
    }
}
