use std::time::Duration;

use tos_common::crypto::Hash;
use tos_tck::tier1_component::TestTransaction;
use tos_tck::tier3_e2e::network::LocalTosNetworkBuilder;

#[tokio::test]
async fn test_business_network_full_chain_path() {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(2)
        .with_genesis_account("alice", 1_000_000)
        .with_genesis_account("bob", 1_000_000)
        .build()
        .await
        .expect("build network");

    let (alice, _) = network.get_genesis_account("alice").unwrap();
    let (bob, _) = network.get_genesis_account("bob").unwrap();

    let tx = TestTransaction {
        hash: Hash::zero(),
        sender: alice.clone(),
        recipient: bob.clone(),
        amount: 10_000,
        fee: 50,
        nonce: 1,
    };

    network
        .submit_and_propagate(0, tx)
        .await
        .expect("submit and propagate tx");

    network
        .mine_and_propagate(0)
        .await
        .expect("mine and propagate block");

    network
        .wait_for_convergence(Duration::from_secs(10))
        .await
        .expect("convergence");

    let balance_node1 = network.node(1).daemon().get_balance(bob).await.unwrap();
    assert_eq!(balance_node1, 1_000_000 + 10_000);
}
