mod common;

use std::borrow::Cow;
use std::sync::Arc;

use common::{create_dummy_block, create_test_storage, setup_account_safe};
use tos_common::{
    account::{CiphertextCache, VersionedUnoBalance},
    asset::{AssetData, VersionedAssetData},
    block::BlockVersion,
    config::{COIN_DECIMALS, UNO_ASSET},
    crypto::{elgamal::KeyPair, proofs::G, Hash},
    transaction::{verify::BlockchainVerificationState, Reference},
    versioned_type::Versioned,
};
use tos_crypto::curve25519_dalek::Scalar;
use tos_daemon::core::{
    error::BlockchainError,
    state::ApplicableChainState,
    storage::{AssetProvider, UnoBalanceProvider},
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_environment::Environment;

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_uno_balance_persistence() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;

    // Register UNO asset for storage lookups
    {
        let mut storage_write = storage.write().await;
        let asset_data = AssetData::new(
            COIN_DECIMALS,
            "UNO".to_string(),
            "UNO".to_string(),
            None,
            None,
        );
        let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
        storage_write.add_asset(&UNO_ASSET, 1, versioned).await?;
    }

    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let sender_pub = sender.get_public_key().compress();
    let receiver_pub = receiver.get_public_key().compress();

    setup_account_safe(&storage, &sender_pub, 0, 0).await?;
    setup_account_safe(&storage, &receiver_pub, 0, 0).await?;

    // Seed sender's UNO balance at topoheight 0
    {
        let mut storage_write = storage.write().await;
        let sender_ct = sender.get_public_key().encrypt(100u64);
        let sender_version =
            VersionedUnoBalance::new(CiphertextCache::Decompressed(sender_ct), None);
        storage_write
            .set_last_uno_balance_to(&sender_pub, &UNO_ASSET, 0, &sender_version)
            .await?;
    }

    // Build a chain state at topoheight 1 and apply a UNO transfer
    let (block, block_hash) = create_dummy_block();
    let executor = Arc::new(TakoContractExecutor::new());
    let environment = Environment::new();

    let mut storage_write = storage.write().await;
    let mut state = ApplicableChainState::new(
        &mut *storage_write,
        &environment,
        0,
        1,
        BlockVersion::Nobunaga,
        0,
        &block_hash,
        &block,
        executor,
    );

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    // Sender spends 25 UNO
    let sender_output = sender.get_public_key().encrypt(25u64);
    state
        .get_sender_uno_balance(&sender_pub, &UNO_ASSET, &reference)
        .await?;
    state
        .add_sender_uno_output(&sender_pub, &UNO_ASSET, sender_output)
        .await?;

    // Receiver gets 25 UNO
    let receiver_ct = receiver.get_public_key().encrypt(25u64);
    let receiver_balance = state
        .get_receiver_uno_balance(Cow::Borrowed(&receiver_pub), Cow::Borrowed(&UNO_ASSET))
        .await?;
    *receiver_balance += receiver_ct;

    state.apply_changes().await?;

    // Verify persisted sender balance
    let (sender_topo, mut sender_version) = storage_write
        .get_last_uno_balance(&sender_pub, &UNO_ASSET)
        .await?;
    assert_eq!(sender_topo, 1);
    let sender_ct = sender_version.get_mut_balance().decompressed()?.clone();
    let sender_point = sender.get_private_key().decrypt_to_point(&sender_ct);
    assert_eq!(sender_point, Scalar::from(75u64) * *G);

    // Verify persisted receiver balance at exact topoheight
    let mut receiver_version = storage_write
        .get_uno_balance_at_exact_topoheight(&receiver_pub, &UNO_ASSET, 1)
        .await?;
    let receiver_ct = receiver_version.get_mut_balance().decompressed()?.clone();
    let receiver_point = receiver.get_private_key().decrypt_to_point(&receiver_ct);
    assert_eq!(receiver_point, Scalar::from(25u64) * *G);

    Ok(())
}
