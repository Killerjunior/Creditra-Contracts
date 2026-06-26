#[cfg(test)]
mod refund_atomicity_tests {
    extern crate std;
    use super::super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::vec::Vec;

    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::{Address, Env, Symbol};

    /// Test: state unchanged when token has insufficient balance for refund
    ///
    /// This test verifies the atomicity of the refund operation when the token
    /// transfer would fail due to insufficient balance. Even though the refund
    /// event is emitted before the transfer, the state remains unchanged when
    /// the transfer fails, and the reentrancy guard is cleared.
    ///
    /// Acceptance Criteria:
    /// - State unchanged on token revert
    /// - Reentrancy guard cleared
    /// - Transaction rolled back atomically
    #[test]
    fn refund_atomicity_on_insufficient_balance() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let contract_id = env.register(Auction, ());
        let client = AuctionClient::new(&env, &contract_id);

        // Register a token with insufficient balance
        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
        let bid_token = token_id.address();

        // Mint only 50 tokens to the contract - not enough to refund 100
        let sac = StellarAssetClient::new(&env, &bid_token);
        sac.mint(&contract_id, &50_i128);

        // Store the bid_token in the auction contract's instance storage
        env.as_contract(&contract_id, || {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "bid_token"), &bid_token);
        });

        let auction_id = Symbol::new(&env, "atomic_refund");
        client.init_auction(
            &auction_id,
            &AuctionMode::English,
            &0,
            &1000,
            &50_i128,
            &0_u32,
            &None,
            &None,
        );

        // Alice places the initial bid of 100
        client.place_bid(&auction_id, &alice, &100_i128);

        // Verify Alice is the highest bidder
        let state_before: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(state_before.highest_bidder.as_ref().unwrap(), &alice);
        assert_eq!(state_before.highest_bid, 100_i128);

        // Bob attempts to outbid with 200
        // The refund transfer will fail due to insufficient balance
        // The transaction should roll back, leaving state unchanged
        let result = catch_unwind(AssertUnwindSafe(|| {
            client.place_bid(&auction_id, &bob, &200_i128);
        }));

        // The operation should have panicked due to insufficient balance for refund
        assert!(result.is_err(), "place_bid should fail when token balance is insufficient");

        // Verify state is unchanged: Alice still the highest bidder with bid 100
        let state_after: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(
            state_after.highest_bidder.as_ref().unwrap(),
            &alice,
            "highest_bidder must be unchanged when refund fails"
        );
        assert_eq!(
            state_after.highest_bid, 100_i128,
            "highest_bid must be unchanged when refund fails"
        );

        // Verify the reentrancy guard is cleared
        let reentrancy_flag: bool = env
            .as_contract(&contract_id, || {
                env.storage()
                    .instance()
                    .get(&Symbol::new(&env, "reentrancy"))
                    .unwrap_or(false)
            });
        assert!(
            !reentrancy_flag,
            "reentrancy guard must be cleared after failed transfer"
        );
    }

    /// Test: outbid with sufficient token balance succeeds and state updates atomically
    ///
    /// This is the positive counterpart to refund_atomicity_on_insufficient_balance.
    /// Verifies that when the token balance is sufficient, state is properly updated.
    #[test]
    fn refund_succeeds_atomically_with_sufficient_balance() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let contract_id = env.register(Auction, ());
        let client = AuctionClient::new(&env, &contract_id);

        // Register a token with sufficient balance
        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
        let bid_token = token_id.address();

        // Mint enough tokens to handle multiple refunds
        let sac = StellarAssetClient::new(&env, &bid_token);
        sac.mint(&contract_id, &10_000_i128);

        // Store the bid_token in instance storage
        env.as_contract(&contract_id, || {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "bid_token"), &bid_token);
        });

        let auction_id = Symbol::new(&env, "atomic_refund_success");
        client.init_auction(
            &auction_id,
            &AuctionMode::English,
            &0,
            &1000,
            &50_i128,
            &0_u32,
            &None,
            &None,
        );

        // Alice places the initial bid
        client.place_bid(&auction_id, &alice, &100_i128);

        // Verify Alice is the highest bidder
        let state_before: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(state_before.highest_bidder.as_ref().unwrap(), &alice);
        assert_eq!(state_before.highest_bid, 100_i128);

        // Bob outbids with 200 - token transfer should succeed
        client.place_bid(&auction_id, &bob, &200_i128);

        // Verify state is updated: Bob is now the highest bidder with bid 200
        let state_after: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(
            state_after.highest_bidder.as_ref().unwrap(),
            &bob,
            "highest_bidder must be updated to Bob after successful outbid"
        );
        assert_eq!(
            state_after.highest_bid, 200_i128,
            "highest_bid must be updated to 200 after successful outbid"
        );

        // Verify the reentrancy guard is cleared
        let reentrancy_flag: bool = env
            .as_contract(&contract_id, || {
                env.storage()
                    .instance()
                    .get(&Symbol::new(&env, "reentrancy"))
                    .unwrap_or(false)
            });
        assert!(
            !reentrancy_flag,
            "reentrancy guard must be cleared after successful refund"
        );
    }

    /// Test: consecutive failed transfers each leave state unchanged
    ///
    /// Verifies that multiple consecutive outbid attempts with an insufficient
    /// token balance each leave the state unchanged, ensuring the atomicity
    /// guarantee holds across multiple failed transactions.
    ///
    /// Acceptance Criteria:
    /// - Each failed bid leaves state unchanged
    /// - Reentrancy guard cleared after each attempt
    /// - Multiple failures don't corrupt state
    #[test]
    fn multiple_failed_transfers_maintain_state_atomicity() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);

        let contract_id = env.register(Auction, ());
        let client = AuctionClient::new(&env, &contract_id);

        // Register a token with insufficient balance for multiple refunds
        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
        let bid_token = token_id.address();

        // Mint only 50 tokens - not enough for even one 100 stroop refund
        let sac = StellarAssetClient::new(&env, &bid_token);
        sac.mint(&contract_id, &50_i128);

        env.as_contract(&contract_id, || {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "bid_token"), &bid_token);
        });

        let auction_id = Symbol::new(&env, "multi_fail_refund");
        client.init_auction(
            &auction_id,
            &AuctionMode::English,
            &0,
            &1000,
            &50_i128,
            &0_u32,
            &None,
            &None,
        );

        // Alice places initial bid of 100
        client.place_bid(&auction_id, &alice, &100_i128);

        // Verify initial state
        let initial_state: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(initial_state.highest_bid, 100_i128);
        assert_eq!(initial_state.highest_bidder.as_ref().unwrap(), &alice);

        // Bob attempts to outbid with 150 - fails
        let bob_attempt = catch_unwind(AssertUnwindSafe(|| {
            client.place_bid(&auction_id, &bob, &150_i128);
        }));
        assert!(bob_attempt.is_err(), "Bob's outbid should fail");

        // Verify state unchanged after Bob's failed attempt
        let state_after_bob: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(state_after_bob.highest_bid, 100_i128);
        assert_eq!(state_after_bob.highest_bidder.as_ref().unwrap(), &alice);

        // Carol attempts to outbid with 200 - also fails
        let carol_attempt = catch_unwind(AssertUnwindSafe(|| {
            client.place_bid(&auction_id, &carol, &200_i128);
        }));
        assert!(carol_attempt.is_err(), "Carol's outbid should fail");

        // Verify state unchanged after Carol's failed attempt
        let state_after_carol: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(state_after_carol.highest_bid, 100_i128);
        assert_eq!(state_after_carol.highest_bidder.as_ref().unwrap(), &alice);

        // Verify reentrancy guard is still clear
        let reentrancy_flag: bool = env
            .as_contract(&contract_id, || {
                env.storage()
                    .instance()
                    .get(&Symbol::new(&env, "reentrancy"))
                    .unwrap_or(false)
            });
        assert!(
            !reentrancy_flag,
            "reentrancy guard must remain cleared after multiple failures"
        );
    }

    /// Test: first bid succeeds without token transfer requirement
    ///
    /// Verifies that the first bid (when no previous bidder exists) does not
    /// trigger a refund and state is properly updated without token transfer.
    /// This documents that the atomicity guarantee applies only to the refund path.
    #[test]
    fn first_bid_succeeds_without_refund_transfer() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);

        let contract_id = env.register(Auction, ());
        let client = AuctionClient::new(&env, &contract_id);
        let auction_id = Symbol::new(&env, "first_bid_no_transfer");

        client.init_auction(
            &auction_id,
            &AuctionMode::English,
            &0,
            &1000,
            &50_i128,
            &0_u32,
            &None,
            &None,
        );

        // Alice places first bid (no refund needed, no token transfer)
        client.place_bid(&auction_id, &alice, &100_i128);

        // Verify state is updated
        let state: crate::types::AuctionState = env
            .as_contract(&contract_id, || env.storage().persistent().get(&auction_id))
            .unwrap();
        assert_eq!(state.highest_bidder.as_ref().unwrap(), &alice);
        assert_eq!(state.highest_bid, 100_i128);

        // Verify reentrancy guard is not set
        let reentrancy_flag: bool = env
            .as_contract(&contract_id, || {
                env.storage()
                    .instance()
                    .get(&Symbol::new(&env, "reentrancy"))
                    .unwrap_or(false)
            });
        assert!(
            !reentrancy_flag,
            "reentrancy guard should not be set for first bid"
        );
    }
}
