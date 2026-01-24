use tos_common::api::daemon::{ForkCondition, HardFork};
use tos_common::block::BlockVersion;
use tos_common::network::Network;
use tos_daemon::core::hard_fork::{
    get_activated_hard_fork, get_version_at_height, get_version_for_state, has_hard_fork_at_height,
    is_hard_fork_activated, is_version_matching_requirement,
};

#[test]
fn test_configured_hard_fork_genesis_activation() {
    let networks = [Network::Mainnet, Network::Testnet, Network::Devnet];
    for network in networks {
        let (is_fork, version) = has_hard_fork_at_height(&network, 0);
        assert!(
            is_fork,
            "expected fork activation at genesis for {:?}",
            network
        );
        assert_eq!(version, BlockVersion::Nobunaga);

        let (is_fork_next, version_next) = has_hard_fork_at_height(&network, 1);
        assert!(!is_fork_next, "no new fork should activate at height 1");
        assert_eq!(version_next, BlockVersion::Nobunaga);

        assert_eq!(get_version_at_height(&network, 0), BlockVersion::Nobunaga);
        assert_eq!(get_version_at_height(&network, 1), BlockVersion::Nobunaga);

        let active = get_activated_hard_fork(&network, 0, 0, 0);
        assert!(active.is_some());
        assert_eq!(active.unwrap().version, BlockVersion::Nobunaga);

        let active_after = get_activated_hard_fork(&network, 1, 1, 1);
        assert!(active_after.is_some());
        assert_eq!(active_after.unwrap().version, BlockVersion::Nobunaga);

        let version_state = get_version_for_state(&network, 1, 1, 1);
        assert_eq!(version_state, BlockVersion::Nobunaga);
    }
}

#[test]
fn test_custom_fork_condition_boundaries() {
    let block_fork = HardFork {
        condition: ForkCondition::Block(10),
        version: BlockVersion::Nobunaga,
        changelog: "test block fork",
        version_requirement: None,
    };
    assert!(!is_hard_fork_activated(&block_fork, 9, 0, 0));
    assert!(is_hard_fork_activated(&block_fork, 10, 0, 0));
    assert!(is_hard_fork_activated(&block_fork, 11, 0, 0));

    let ts_fork = HardFork {
        condition: ForkCondition::Timestamp(1_700_000_000_000),
        version: BlockVersion::Nobunaga,
        changelog: "test timestamp fork",
        version_requirement: None,
    };
    assert!(!is_hard_fork_activated(&ts_fork, 0, 1_699_999_999_999, 0));
    assert!(is_hard_fork_activated(&ts_fork, 0, 1_700_000_000_000, 0));

    let tcd_fork = HardFork {
        condition: ForkCondition::TCD(1_000_000),
        version: BlockVersion::Nobunaga,
        changelog: "test tcd fork",
        version_requirement: None,
    };
    assert!(!is_hard_fork_activated(&tcd_fork, 0, 0, 999_999));
    assert!(is_hard_fork_activated(&tcd_fork, 0, 0, 1_000_000));

    let never_fork = HardFork {
        condition: ForkCondition::Never,
        version: BlockVersion::Nobunaga,
        changelog: "test never fork",
        version_requirement: None,
    };
    assert!(!is_hard_fork_activated(
        &never_fork,
        u64::MAX,
        u64::MAX,
        u64::MAX
    ));
}

#[test]
fn test_future_fork_version_requirement_matching() {
    assert!(is_version_matching_requirement("1.2.3", ">=1.2.0").unwrap());
    assert!(is_version_matching_requirement("1.2.3-abcdef", ">=1.2.0").unwrap());
    assert!(!is_version_matching_requirement("1.1.9", ">=1.2.0").unwrap());
}
