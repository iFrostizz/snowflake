// use criterion::{black_box, criterion_group, criterion_main, Criterion};
// use flume::Sender;
// use snowflake::id::NodeId;
// use snowflake::server::{
//     config,
//     peers::{Network, PeerMessage},
//     Server,
// };
// use std::sync::Arc;
// use std::time::Instant;

// async fn run_server(
//     spn: Sender<(NodeId, PeerMessage)>,
//     transaction_tx: Sender<(Vec<u8>, Instant)>,
// ) {
//     let (exit_tx, exit_rx) = flume::unbounded();

//     let network = Arc::new(
//         Network::new(
//             public_socket,
//             network_id,
//             chain_id,
//             &key_path,
//             &cert_path,
//             chain_state,
//             db_tx.clone(),
//         )
//         .unwrap(),
//     );

//     let server_config = config::server_config(&cert_path, &key_path);
//     let server_config = Arc::new(server_config);

//     let server = tokio::task::spawn(async move {
//         // tokio::select! {
//         //     _ = exit_rx.recv_async() => {
//         //         return;
//         //     }
//         // }

//         Server::start(network, server_config, spn, transaction_tx).await
//     });
// }

// pub fn criterion_benchmark(c: &mut Criterion) {
//     let (spn, rpn) = flume::unbounded();
//     let (transaction_tx, transaction_rx) = flume::unbounded();

//     c.bench_function("signed_tx_e2e", |b| b.iter(|| fibonacci(black_box(20))));
// }

// criterion_group!(benches, criterion_benchmark);
// criterion_main!(benches);
