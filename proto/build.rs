// Copyright 2019 TiKV Project Authors. Licensed under Apache-2.0.

fn main() {
    tonic_build::configure()
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile(&["proto/eraftpb.proto"], &["proto"])
        .unwrap()
}
