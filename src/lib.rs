#![allow(dead_code)]
#![allow(unused_variables)]


extern crate core;

use concordium_std::*;
use core::fmt::Debug;

/// # Implementation of an auction smart contract
///
/// To bid, participants send CCD using the bid function.
/// The participant with the highest bid wins the auction.
/// Bids are to be placed before the auction end. After that, bids are refused.
/// Only bids that exceed the highest bid are accepted.
/// Bids are placed incrementally, i.e., an account's bid is considered
/// to be the **sum** of all bids.
///
/// Example: if Alice first bid 1 CCD and then bid 2 CCD, her total
/// bid is 3 CCD. The bidding will only go through if 3 CCD is higher than
/// the currently highest bid.
///
/// After the auction end, any account can finalize the auction.
/// The auction can be finalized only once.
/// When the auction is finalized, every participant except the
/// winner gets their money back.

/// The state in which an auction can be.
#[derive(Debug, Serialize, SchemaType, Eq, PartialEq, PartialOrd)]
pub enum AuctionState {
    /// The auction is either
    /// - still accepting bids or
    /// - not accepting bids because it's past the auction end, but nobody has
    ///   finalized the auction yet.
    NotSoldYet,
    /// The auction is over and the item has been sold to the indicated address.
    Sold(AccountAddress), // winning account's address
}

/// The state of the smart contract.
/// This is the state that will be shown when the contract is queried using
/// `concordium-client contract show`.
#[derive(Debug, Serial, DeserialWithState)]
#[concordium(state_parameter = "S")]
pub struct State<S> {
    /// Has the item been sold?
    auction_state: AuctionState,
    /// The highest bid so far (stored explicitly so that bidders can quickly
    /// see it)
    highest_bid:   Amount,
    /// The sold item (to be displayed to the auction participants).
    item:          String,
    /// Expiration time of the auction at which bids will be closed (to be
    /// displayed to the auction participants)
    expiry:        Timestamp,
    /// Keeping track of which account bid how much money
    bids:          StateMap<AccountAddress, Amount, S>,
}

/// Type of the parameter to the `init` function.
#[derive(Serialize, SchemaType)]
struct InitParameter {
    /// The item to be sold.
    item:   String,
    /// Time of the auction end in the RFC 3339 format (https://tools.ietf.org/html/rfc3339)
    expiry: Timestamp,
}

/// For errors in which the `bid` function can result
#[derive(Debug, PartialEq, Eq, Clone, Reject)]
enum BidError {
    ContractSender, // raised if a contract, as opposed to account, tries to bid
    BidTooLow,      /* { bid: Amount, highest_bid: Amount } */
    // raised if bid is lower than highest amount
    BidsOverWaitingForAuctionFinalization, // raised if bid is placed after auction expiry time
    AuctionFinalized,                      /* raised if bid is placed after auction has been
                                            * finalized */
}

/// For errors in which the `finalize` function can result
#[derive(Debug, PartialEq, Eq, Clone, Reject)]
enum FinalizeError {
    BidMapError,        /* raised if there is a mistake in the bid map that keeps track of all
                         * accounts' bids */
    AuctionStillActive, // raised if there is an attempt to finalize the auction before its expiry
    AuctionFinalized,   // raised if there is an attempt to finalize an already finalized auction
}

/// Init function that creates a new auction
#[init(contract = "auction", parameter = "InitParameter")]
fn auction_init<S: HasStateApi>(
    ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<State<S>> {
    let parameter: InitParameter = ctx.parameter_cursor().get()?;
    let state = State {
        auction_state: AuctionState::NotSoldYet,
        highest_bid:   Amount::zero(),
        item:          parameter.item,
        expiry:        parameter.expiry,
        bids:          state_builder.new_map(),
    };
    Ok(state)
}

/// Receive function in which accounts can bid before the auction end time
#[receive(contract = "auction", name = "bid", payable, mutable)]
fn auction_bid<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
    amount: Amount,
) -> Result<(), BidError> {
    let state = host.state_mut();
    ensure!(state.auction_state == AuctionState::NotSoldYet, BidError::AuctionFinalized);

    let slot_time = ctx.metadata().slot_time();
    ensure!(slot_time <= state.expiry, BidError::BidsOverWaitingForAuctionFinalization);

    let sender_address = match ctx.sender() {
        Address::Contract(_) => bail!(BidError::ContractSender),
        Address::Account(account_address) => account_address,
    };
    let mut bid_to_update = state.bids.entry(sender_address).or_insert(Amount::zero());
    //println!("[before] bid_to_update: {}", (*bid_to_update).micro_ccd);
    //println!("[before] state.highest_bid: {}", state.highest_bid.micro_ccd);
    *bid_to_update += amount;
    //println!("[after] state.highest_bid: {}", state.highest_bid.micro_ccd);
    //println!("[after] bid_to_update: {}", (*bid_to_update).micro_ccd);
    //println!("[auction_bid] bidTooLow: {} > {}", (*bid_to_update).micro_ccd, state.highest_bid.micro_ccd);
    // Ensure that the new bid exceeds the highest bid so far
    ensure!(*bid_to_update > state.highest_bid, BidError::BidTooLow);

    state.highest_bid = *bid_to_update;

    Ok(())
}

/// Receive function used to finalize the auction, returning all bids to their
/// senders, except for the winning bid
#[receive(contract = "auction", name = "finalize", mutable)]
fn auction_finalize<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> Result<(), FinalizeError> {
    let state = host.state();
    ensure!(state.auction_state == AuctionState::NotSoldYet, FinalizeError::AuctionFinalized);

    let slot_time = ctx.metadata().slot_time();
    println!("slot_time > state.expire _____  {} > {}", slot_time.timestamp_millis(), state.expiry.timestamp_millis());
    ensure!(slot_time > state.expiry, FinalizeError::AuctionStillActive);

    let owner = ctx.owner();

    println!("A");
    let balance = host.self_balance();
    if balance == Amount::zero() {
        println!("A2");
        Ok(())
    } else {
        println!("B");
        if host.invoke_transfer(&owner, state.highest_bid).is_err() {
            bail!(FinalizeError::BidMapError);
        }
        println!("C");
        let mut remaining_bid = None;
        // Return bids that are smaller than highest
        for (addr, amnt) in state.bids.iter() {
            println!("D");
            if *amnt < state.highest_bid {
                println!("transfer to: {:?},  amount: {}", addr.0[1], (*amnt).micro_ccd);
                if host.invoke_transfer(&*addr, *amnt).is_err() {
                    bail!(FinalizeError::BidMapError);
                    //host.
                }
            } else {
                ensure!(remaining_bid.is_none(), FinalizeError::BidMapError);
                remaining_bid = Some((addr, amnt));
            }
        }
        // Ensure that the only bidder left in the map is the one with the highest bid
        match remaining_bid {
            Some((addr, amount)) => {
                ensure!(*amount == state.highest_bid, FinalizeError::BidMapError);
                host.state_mut().auction_state = AuctionState::Sold(*addr);
                Ok(())
            }
            None => bail!(FinalizeError::BidMapError),
        }
    }
}

#[concordium_cfg_test]
mod tests {
    use std::borrow::{Borrow, BorrowMut};
    use std::cmp::min;
    use std::ops::Range;
    use super::*;
    use concordium_std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::thread::sleep;
    use std::{process, time};
    use test_infrastructure::*;

    // A counter for generating new account addresses
    static ADDRESS_COUNTER: AtomicU8 = AtomicU8::new(0);
    const AUCTION_END: u64 = 1;
    const ITEM: &str = "Starry night by Van Gogh";

    fn expect_error<E, T>(expr: Result<T, E>, err: E, msg: &str)
        where
            E: Eq + Debug,
            T: Debug, {
        let actual = expr.expect_err(msg);
        assert_eq!(actual, err);
    }

    fn item_expiry_parameter() -> InitParameter {
        InitParameter {
            item:   ITEM.into(),
            expiry: Timestamp::from_timestamp_millis(AUCTION_END),
        }
    }

    fn create_parameter_bytes(parameter: &InitParameter) -> Vec<u8> { to_bytes(parameter) }

    fn parametrized_init_ctx<'a>(parameter_bytes: &'a Vec<u8>) -> TestInitContext<'a> {
        let mut ctx = TestInitContext::empty();
        ctx.set_parameter(parameter_bytes);
        ctx
    }

    fn new_account() -> AccountAddress {
        let account = AccountAddress([ADDRESS_COUNTER.load(Ordering::SeqCst); 32]);
        ADDRESS_COUNTER.fetch_add(1, Ordering::SeqCst);
        account
    }

    fn new_account_ctx<'a>() -> (AccountAddress, TestReceiveContext<'a>) {
        let account = new_account();
        let ctx = new_ctx(account, account, AUCTION_END);
        (account, ctx)
    }

    fn new_ctx<'a>(
        owner: AccountAddress,
        sender: AccountAddress,
        slot_time: u64,
    ) -> TestReceiveContext<'a> {
        let mut ctx = TestReceiveContext::empty();
        ctx.set_sender(Address::Account(sender));
        ctx.set_owner(owner);
        ctx.set_metadata_slot_time(Timestamp::from_timestamp_millis(slot_time));
        ctx
    }

    #[concordium_test]
    /// Test that the smart-contract initialization sets the state correctly
    /// (no bids, active state, indicated auction-end time and item name).
    fn test_init() {
        let parameter_bytes = create_parameter_bytes(&item_expiry_parameter());
        let ctx = parametrized_init_ctx(&parameter_bytes);

        let mut state_builder = TestStateBuilder::new();

        let state_result = auction_init(&ctx, &mut state_builder);
        state_result.expect_report("Contract initialization results in error");
    }

    #[concordium_test]
    /// Test a sequence of bids and finalizations:
    /// 0. Auction is initialized.
    /// 1. Alice successfully bids 0.1 CCD.
    /// 2. Alice successfully bids another 0.1 CCD, highest bid becomes 0.2 CCD
    /// (the sum of her two bids). 3. Bob successfully bids 0.3 CCD, highest
    /// bid becomes 0.3 CCD. 4. Someone tries to finalize the auction before
    /// its end time. Attempt fails. 5. Dave successfully finalizes the
    /// auction after its end time.    Alice gets her money back, while
    /// Carol (the owner of the contract) collects the highest bid amount.
    /// 6. Attempts to subsequently bid or finalize fail.
    fn test_auction_bid_and_finalize() {
        let parameter_bytes = create_parameter_bytes(&item_expiry_parameter());
        let ctx0 = parametrized_init_ctx(&parameter_bytes);

        let amount = Amount::from_micro_ccd(100);
        let winning_amount = Amount::from_micro_ccd(300);
        let big_amount = Amount::from_micro_ccd(500);

        let mut state_builder = TestStateBuilder::new();
        let mut bid_map = BTreeMap::new();

        // initializing auction
        let initial_state =
            auction_init(&ctx0, &mut state_builder).expect("Initialization should pass");

        let mut host = TestHost::new(initial_state, state_builder);

        // 1st bid: account1 bids amount1
        let (alice, alice_ctx) = new_account_ctx();
        verify_bid(&mut host, alice, &alice_ctx, amount, &mut bid_map, amount);

        // 2nd bid: account1 bids `amount` again
        // should work even though it's the same amount because account1 simply
        // increases their bid
        verify_bid(&mut host, alice, &alice_ctx, amount, &mut bid_map, amount + amount);

        // 3rd bid: second account
        let (bob, bob_ctx) = new_account_ctx();
        verify_bid(&mut host, bob, &bob_ctx, winning_amount, &mut bid_map, winning_amount);

        // trying to finalize auction that is still active
        // (specifically, the bid is submitted at the last moment, at the AUCTION_END
        // time)
        let mut ctx4 = TestReceiveContext::empty();
        ctx4.set_metadata_slot_time(Timestamp::from_timestamp_millis(AUCTION_END));
        let finres = auction_finalize(&ctx4, &mut host);
        expect_error(
            finres,
            FinalizeError::AuctionStillActive,
            "Finalizing auction should fail when it's before auction-end time",
        );

        // finalizing auction
        let carol = new_account();
        let dave = new_account();
        let ctx5 = new_ctx(carol, dave, AUCTION_END + 1);
        host.set_self_balance(winning_amount + amount + amount);
        let finres2 = auction_finalize(&ctx5, &mut host);
        finres2.expect_report("Finalizing auction should work");
        let transfers = host.get_transfers();
        claim_eq!(&transfers[..], &[(carol, winning_amount), (alice, amount + amount)]);
        claim_eq!(host.state().auction_state, AuctionState::Sold(bob));
        claim_eq!(host.state().highest_bid, winning_amount);

        // attempting to finalize auction again should fail
        let finres3 = auction_finalize(&ctx5, &mut host);
        expect_error(
            finres3,
            FinalizeError::AuctionFinalized,
            "Finalizing auction a second time should fail",
        );

        // attempting to bid again should fail
        let res4 = auction_bid(&bob_ctx, &mut host, big_amount);
        expect_error(
            res4,
            BidError::AuctionFinalized,
            "Bidding should fail because the auction is finalized",
        );
    }

    fn verify_bid(
        host: &mut TestHost<State<TestStateApi>>,
        account: AccountAddress,
        ctx: &TestContext<TestReceiveOnlyData>,
        amount: Amount,
        bid_map: &mut BTreeMap<AccountAddress, Amount>,
        highest_bid: Amount,
    ) {
        auction_bid(ctx, host, amount).expect_report("Bidding should pass.");
        bid_map.insert(account, highest_bid);
    }

    #[concordium_test]
    /// Bids for amounts lower or equal to the highest bid should be rejected.
    fn test_auction_bid_repeated_bid() {
        let (account1, ctx1) = new_account_ctx();
        let ctx2 = new_account_ctx().1;

        let parameter_bytes = create_parameter_bytes(&item_expiry_parameter());
        let ctx0 = parametrized_init_ctx(&parameter_bytes);

        let amount = Amount::from_micro_ccd(100);

        let mut state_builder = TestStateBuilder::new();
        let mut bid_map = BTreeMap::new();

        // initializing auction
        let initial_state =
            auction_init(&ctx0, &mut state_builder).expect("Initialization should succeed.");

        let mut host = TestHost::new(initial_state, state_builder);

        // 1st bid: account1 bids amount1
        verify_bid(&mut host, account1, &ctx1, amount, &mut bid_map, amount);

        // 2nd bid: account2 bids amount1
        // should fail because amount is equal to highest bid
        let res2 = auction_bid(&ctx2, &mut host, amount);
        expect_error(
            res2,
            BidError::BidTooLow,
            "Bidding 2 should fail because bid amount must be higher than highest bid",
        );
    }

    #[concordium_test]
    /// Bids for 0 CCD should be rejected.
    fn test_auction_bid_zero() {
        let ctx1 = new_account_ctx().1;
        let parameter_bytes = create_parameter_bytes(&item_expiry_parameter());
        let ctx = parametrized_init_ctx(&parameter_bytes);

        let mut state_builder = TestStateBuilder::new();

        // initializing auction
        let initial_state =
            auction_init(&ctx, &mut state_builder).expect("Initialization should succeed.");

        let mut host = TestHost::new(initial_state, state_builder);

        let res = auction_bid(&ctx1, &mut host, Amount::zero());
        expect_error(res, BidError::BidTooLow, "Bidding zero should fail");
    }



    ///QUICKCHECK

    use quickcheck::{Gen, Arbitrary, Testable};
    use quickcheck_macros::quickcheck;
    use rand::{Rng, SeedableRng, thread_rng};
    use crate::schema::SizeLength::U64;

    #[derive(Debug)]
    pub struct AmountFixture(pub Amount);
    //clone values
    impl Clone for AmountFixture {
        fn clone(&self) -> Self { AmountFixture(self.0) }
    }

    //make arbitrary values
    impl Arbitrary for AmountFixture {
        fn arbitrary(g: &mut Gen) -> Self {
            Self(concordium_std::Amount { micro_ccd: g.size() as u64 })
        }
    }

    #[derive(Debug)]
    pub struct Bids(pub Vec<u64>);

    impl Clone for Bids {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl Arbitrary for Bids {
        fn arbitrary(g: &mut Gen) -> Self {
            Bids((1..10).map(|_| (u64::arbitrary(g) % 10) + 1).collect())
        }
    }


    #[quickcheck]
    fn propertybased(amount: AmountFixture, bids: Bids, account_to_bid: Vec<u8>) -> bool {

        println!("starting propertybased");
        println!("BIDS LENGTH: {}", bids.0.len());

        let number_of_bids = account_to_bid.len();
        if number_of_bids <= 1 {
            println!("Returned true");
            return true
        }


        let parameter_bytes = create_parameter_bytes(&item_expiry_parameter());
        let ctx0 = parametrized_init_ctx(&parameter_bytes);
        let mut state_builder = TestStateBuilder::new();
        let initial_state =
            auction_init(&ctx0, &mut state_builder).expect("Initialization should pass");
        let mut host = TestHost::new(initial_state, state_builder);

        let mut accounts: BTreeMap<u8, TestReceiveContext> = BTreeMap::new(); ///account#, account
        let mut bid_map: BTreeMap<AccountAddress, Amount> = BTreeMap::new();
        let highest_bid = Amount { micro_ccd: u64::MIN };

        ///generate accounts and insert them into map
        for accNum in account_to_bid.clone() {
            let account = new_account();
            accounts.insert(accNum, new_ctx(account, account, 0));
        }


        let mut global_highest_bid = 0;
        let mut address_of_last_buyer: AccountAddress = new_account();
        ///perform a bid for each number in the list of "number_of_bids"
        for i in 0..(min(number_of_bids, bids.0.len())) {
            println!("Inserting into bit_map for account: {}", account_to_bid[i]);
            //println!("         ");
            //println!("INDEX: {}", i);
            assert_ne!(bids.0[0], 0);


            let ctx: &TestReceiveContext = accounts.get(&account_to_bid[i]).unwrap();
            address_of_last_buyer = ctx.owner();

            let highest_bid = bid_map.get(&ctx.owner()).cloned().unwrap_or(Amount { micro_ccd: 0 }).micro_ccd;

            //println!("bid: {}, global_high: {}, sum: {}", bids.0[i], global_highest_bid, global_highest_bid + bids.0[i]);
            assert!(bids.0[i] > 0);
            //global_highest_bid += (bids.0[i]);
            // 50 -> 50
            // 10 -> 60 (110)
            //println!("[after] bid: {}, global_high: {}, sum: {}", bids.0[i], global_highest_bid, global_highest_bid + bids.0[i]);
            global_highest_bid = host.borrow_mut().state().highest_bid.micro_ccd + bids.0[i];
            verify_bid(&mut host,
                       ctx.owner(),
                       &ctx,
                       Amount { micro_ccd: global_highest_bid },
                       &mut bid_map,
                       Amount { micro_ccd: global_highest_bid }
            );

        }


        sleep(time::Duration::from_millis(10));

        host.borrow_mut().set_self_balance(Amount { micro_ccd: u64::MAX });
        let ctx: TestReceiveContext = new_ctx(new_account(), new_account(), 2);
        let result = auction_finalize(&ctx, &mut host);
        //result.err()?;

        if result.is_err() {
            eprintln!("{:?}", result.err().unwrap());
            process::exit(1);
        }


        // Last person to bid should have highest bid
        let auction_winner: AccountAddress = address_of_last_buyer;


        let transfers = host.borrow_mut().get_transfers();
        //println!("{}", transfers.into_iter());
        assert!(transfers.len() > 0);
        println!("transfers: {:?}", transfers);
        println!("bid_map: {:?}", bid_map);

        for (acc, amount) in transfers {
            println!("Transfer to account: {}", acc.0[1]);

            //check winner is correct and that they got nothing back
            if acc == auction_winner {
                assert_eq!(amount, Amount { micro_ccd: 0 });
            }
            //else check that other accounts got back what they bid
            else {
                // let highest_bid = bid_map.get(&ctx.owner()).cloned().unwrap_or(Amount { micro_ccd: 0 }).micro_ccd;
                //bid_map.get(&acc).unwrap();
                if bid_map.get(&acc).unwrap_or(&Amount {micro_ccd: -25} ).micro_ccd == -25 {
                    println!("default case hit")
                }
                assert_eq!(amount.micro_ccd, bid_map.get(&acc).unwrap_or(&Amount {micro_ccd: 0} ).micro_ccd );
            }
        }

        true
    }


}