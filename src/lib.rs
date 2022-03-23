#![allow(dead_code)]
#![allow(unused_variables)]
use concordium_std::{collections::BTreeMap, *};
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
#[contract_state(contract = "auction")]
#[derive(Debug, Serialize, SchemaType, Eq, PartialEq)]
pub struct State {
    /// Has the item been sold?
    auction_state: AuctionState,
    /// The highest bid so far (stored explicitly so that bidders can quickly
    /// see it)
    highest_bid:   Amount,
    /// The sold item (to be displayed to the auction participants), encoded in
    /// ASCII
    item:          Vec<u8>,
    /// Expiration time of the auction at which bids will be closed (to be
    /// displayed to the auction participants)
    expiry:        Timestamp,
    /// Keeping track of which account bid how much money
    #[concordium(size_length = 2)]
    bids:          BTreeMap<AccountAddress, Amount>,
}

/// A helper function to create a state for a new auction.
fn fresh_state(itm: Vec<u8>, exp: Timestamp) -> State {
    State {
        auction_state: AuctionState::NotSoldYet,
        highest_bid:   Amount::zero(),
        item:          itm,
        expiry:        exp,
        bids:          BTreeMap::new(),
    }
}

/// Type of the parameter to the `init` function.
#[derive(Serialize, SchemaType)]
struct InitParameter {
    /// The item to be sold, as a sequence of ASCII codes.
    item:   Vec<u8>,
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
fn auction_init(ctx: &impl HasInitContext) -> InitResult<State> {
    let parameter: InitParameter = ctx.parameter_cursor().get()?;
    Ok(fresh_state(parameter.item, parameter.expiry))
}

/// Receive function in which accounts can bid before the auction end time
#[receive(contract = "auction", name = "bid", payable)]
fn auction_bid<A: HasActions>(
    ctx: &impl HasReceiveContext,
    amount: Amount,
    state: &mut State,
) -> Result<A, BidError> {
    ensure!(state.auction_state == AuctionState::NotSoldYet, BidError::AuctionFinalized);

    let slot_time = ctx.metadata().slot_time();
    ensure!(slot_time <= state.expiry, BidError::BidsOverWaitingForAuctionFinalization);

    let sender_address = match ctx.sender() {
        Address::Contract(_) => bail!(BidError::ContractSender),
        Address::Account(account_address) => account_address,
    };
    let bid_to_update = state.bids.entry(sender_address).or_insert_with(Amount::zero);

    *bid_to_update += amount;
    // Ensure that the new bid exceeds the highest bid so far
    ensure!(
        *bid_to_update > state.highest_bid,
        BidError::BidTooLow /* { bid: amount, highest_bid: state.highest_bid } */
    );
    state.highest_bid = *bid_to_update;

    Ok(A::accept())
}

/// Receive function used to finalize the auction, returning all bids to their
/// senders, except for the winning bid
#[receive(contract = "auction", name = "finalize")]
fn auction_finalize<A: HasActions>(
    ctx: &impl HasReceiveContext,
    state: &mut State,
) -> Result<A, FinalizeError> {
    ensure!(state.auction_state == AuctionState::NotSoldYet, FinalizeError::AuctionFinalized);

    let slot_time = ctx.metadata().slot_time();
    ensure!(slot_time > state.expiry, FinalizeError::AuctionStillActive);

    let owner = ctx.owner();

    let balance = ctx.self_balance();
    if balance == Amount::zero() {
        Ok(A::accept())
    } else {
        let mut return_action = A::simple_transfer(&owner, state.highest_bid);
        let mut remaining_bid = None;
        // Return bids that are smaller than highest
        for (addr, &amnt) in state.bids.iter() {
            if amnt < state.highest_bid {
                return_action = return_action.and_then(A::simple_transfer(addr, amnt));
            } else {
                ensure!(remaining_bid.is_none(), FinalizeError::BidMapError);
                state.auction_state = AuctionState::Sold(*addr);
                remaining_bid = Some((addr, amnt));
            }
        }
        // Ensure that the only bidder left in the map is the one with the highest bid
        match remaining_bid {
            Some((_, amount)) => {
                ensure!(amount == state.highest_bid, FinalizeError::BidMapError);
                Ok(return_action)
            }
            None => bail!(FinalizeError::BidMapError),
        }
    }
}



use quickcheck::{Gen, Arbitrary};


#[cfg(test)]
extern crate quickcheck;
#[cfg(test)]
#[macro_use(quickcheck)]
extern crate quickcheck_macros;


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    use quickcheck::TestResult;
    use test_infrastructure::*;

    //extern crate quickcheck_macros;
    //extern crate quickcheck;

    #[derive(Debug)]
    pub struct AmountFixture(pub Amount);

    impl Clone for AmountFixture {
        fn clone(&self) -> Self {
            AmountFixture(self.0)
        }
    }

    impl Arbitrary for AmountFixture {
        fn arbitrary(g: &mut Gen) -> Self {
            Self(concordium_std::Amount { micro_ccd: g.size() as u64 })
        }
    }

    #[quickcheck]
    fn propertybased(amount: AmountFixture, timestamp_millis: u64) -> bool {
    //fn propertybased(amount: AmountFixture) -> bool {
        let (bob, bob_ctx) = new_account_ctx();
        println!("{}", timestamp_millis);

        if timestamp_millis == 0 {
            return true;
        }


        let state = &mut State {
            auction_state: AuctionState::NotSoldYet,
            highest_bid: Amount { micro_ccd: amount.0.micro_ccd },
            item: vec![],
            expiry: Timestamp::from_timestamp_millis(timestamp_millis),
            bids: Default::default()
        };

        let result: Result<ActionsTree, BidError> = auction_bid(&bob_ctx, Amount { micro_ccd: amount.0.micro_ccd + 1 }, state);
        println!("{:?}", result.as_ref().err());

        result.is_ok()
    }

    #[quickcheck]
    fn this_is_it(amount: AmountFixture) -> bool {
        //const AUCTION_END: u64 = 1;
        //let carol = new_account();
        //let dave = new_account();
        //let mut ctx = new_ctx(carol, dave, AUCTION_END + 1);
        let (bob, bob_ctx) = new_account_ctx();

        let state = &mut State {
            auction_state: AuctionState::NotSoldYet,
            highest_bid: Amount { micro_ccd: 0 },
            item: vec![],
            expiry: Timestamp::from_timestamp_millis(1),
            bids: Default::default()
        };

        let result: Result<ActionsTree, BidError> = auction_bid(&bob_ctx, Amount { micro_ccd: amount.0.micro_ccd }, state);

        result.is_ok()
    }



    // A counter for generating new account addresses
    static ADDRESS_COUNTER: AtomicU8 = AtomicU8::new(0);
    const AUCTION_END: u64 = 1;
    const ITEM: &str = "Starry night by Van Gogh";

    fn dummy_fresh_state() -> State { dummy_active_state(Amount::zero(), BTreeMap::new()) }

    fn dummy_active_state(highest: Amount, bids: BTreeMap<AccountAddress, Amount>) -> State {
        State {
            auction_state: AuctionState::NotSoldYet,
            highest_bid: highest,
            item: ITEM.as_bytes().to_vec(),
            expiry: Timestamp::from_timestamp_millis(AUCTION_END),
            bids,
        }
    }

    fn expect_error<E, T>(expr: Result<T, E>, err: E, msg: &str)
        where
            E: Eq + Debug,
            T: Debug, {
        let actual = expr.expect_err(msg);
        assert_eq!(actual, err);
    }

    fn item_expiry_parameter() -> InitParameter {
        InitParameter {
            item: ITEM.as_bytes().to_vec(),
            expiry: Timestamp::from_timestamp_millis(AUCTION_END),
        }
    }

    fn create_parameter_bytes(parameter: &InitParameter) -> Vec<u8> { to_bytes(parameter) }

    fn parametrized_init_ctx<'a>(parameter_bytes: &'a Vec<u8>) -> InitContextTest<'a> {
        let mut ctx = InitContextTest::empty();
        ctx.set_parameter(parameter_bytes);
        ctx
    }

    fn new_account() -> AccountAddress {
        let account = AccountAddress([ADDRESS_COUNTER.load(Ordering::SeqCst); 32]);
        ADDRESS_COUNTER.fetch_add(1, Ordering::SeqCst);
        account
    }

    fn new_account_ctx<'a>() -> (AccountAddress, ReceiveContextTest<'a>) {
        let account = new_account();
        let ctx = new_ctx(account, account, AUCTION_END);
        (account, ctx)
    }

    fn new_ctx<'a>(
        owner: AccountAddress,
        sender: AccountAddress,
        slot_time: u64,
    ) -> ReceiveContextTest<'a> {
        let mut ctx = ReceiveContextTest::empty();
        ctx.set_sender(Address::Account(sender));
        ctx.set_owner(owner);
        ctx.set_metadata_slot_time(Timestamp::from_timestamp_millis(slot_time));
        ctx
    }


}
