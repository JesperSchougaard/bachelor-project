#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(warnings, unused)]

extern crate core;
extern crate crypto;
use self::crypto::digest::Digest;
use self::crypto::sha3::Sha3;

use concordium_std::*;
use core::fmt::Debug;
use std::borrow::Borrow;
use std::io::Bytes;
use bytes32::Bytes32;
use crate::schema::Type::Timestamp;
use crate::State::Requested;

#[derive(Serialize, SchemaType)]
struct InitParameter {
    // address of whoever is the "vendor" (should be seller)
    vendor: AccountAddress,
    // length of time the smart contract runs for??
    timeout: u64,

    // publicKey = _publicKey // not needed maybe

}


/// `concordium-client contract show`.
#[derive(Debug)]
pub enum State {
    Null,
    Requested,
    Accepted,
    Rejected,
    Delivered,
    Completed,
    Dispute,
    Counter,
    Failed,
}


#[derive(Serial, SchemaType)]
struct Item {
    item_value: u64,
    description: String,
}


/// A single purchase by a buyer.
#[derive(Debug, Serial)]
struct Purchase {
    commit: bytes32,         // Commitment to buyer random bit
    timestamp: Timestamp,   // The last block where activity was recorded (for timeouts).
    item: u64,               // Identifier of the item purchased.
    seller_bit: bool,        // Seller random bit

    notes: String,           // Buyer notes about purchase (shipping etc.)
    state: State,            // Current state of the purchase.

    buyer: AccountAddress,   // Address of the buyer
}

pub struct ContractState<S> {
    vendor: AccountAddress,
    timeout: u64,
    state: State,
    contracts: StateMap<bytes32, Purchase, S>,
    listings: StateMap<u64, Item, S>,
}


#[init(contract = "vendor", parameter = "InitParameter")]
fn init<S: HasStateApi>(
    _ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<ContractState<S>> {
    let parameter: InitParameter = ctx.parameter_cursor().get()?;
    ensure!(parameter.timeout > 0);
    let state = ContractState {
        state: State::Null,
        vendor: parameter.vendor,
        timeout: parameter.timeout,
        contracts: state_builder.new_map(), //HashMap::new(),
        listings: state_builder.new_map(),
    };
    Ok(state)
}

// BUYER  FUNCTIONS

#[receive(contract = "vendor", name = "Request_purchase", payable)]
fn buyerRequestPurchase<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<S>, StateApiType = S>,
    item: u64,
    info: String,
    amount: Amount,
    timestamp: u64,
) -> Result<String, E> {
    ensure!(amount == ContractState.listings.entry(item).item_value); //must pay correct amount
    let mut hasher = Sha3::keccak256();

    hasher.input_str();
    let id: String = hasher.result_str();

    //Bytes32 { store: vec![] }
    //ContractState.contracts[id] = Purchase {
    ContractState.contracts.insert(id, Purchase {
        commit: 0x0,
        timestamp: Timestamp::from_timestamp_millis(timestamp),
        item: *item, // * star needed?
        seller_bit: False,
        notes: info,
        state: Requested,
        buyer: ctx.sender.address,
    });
    Ok(id.clone())
}

#[receive(contract = "vendor", name = "buyer_Abort")]
fn buyer_Abort<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<S>, StateApiType = S>,
    id: bytes32,
) {
    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    // Only the buyer can abort the contract.
    ensure!(ctx.sender.address == contract.buyer, "only the buyer can abort the contract");
    ensure!(contract.state == Requested,
        "Can only abort contract before vendor has interacted with contract");

    contract.state = State.Failed;
    host.invoke_transfer(&contract.buyer, address(this).balance);
    host.invoke_transfer(&contract.buyer, ContractState.listings.entry(item).item_value);
    // TODO: what??
    contracts.buyer.transfer(address(this).balance);            // Return money to buyer
}


#[receive(contract = "vendor", name = "buyer_ConfirmDelivery")]
fn buyer_ConfirmDelivery(id: bytes32) {

}


#[receive(contract = "vendor", name = "buyer_DisputeDelivery", payable)]
fn buyer_DisputeDelivery(id: bytes32, commitment: bytes32 ) {

}


#[receive(contract = "vendor", name = "buyer_CallTimeout")]
fn buyer_CallTimeout(id: bytes32) {

}


#[receive(contract = "vendor", name = "buyer_OpenCommitment")]
fn buyer_OpenCommitment(id: bytes32, _buyerBit: bool, nonce: bytes32) {

}




// SELLER FUNCTIONS

#[receive(contract = "vendor", name = "seller_CallTimeout")]
fn seller_CallTimeout(id: bytes32) {

}


#[receive(contract = "vendor", name = "seller_RejectContract")]
fn seller_RejectContract(id: bytes32) {

}


#[receive(contract = "vendor", name = "seller_AcceptContract")]
fn seller_AcceptContract(id: bytes32) {

}


#[receive(contract = "vendor", name = "seller_ItemWasDelivered")]
fn seller_ItemWasDelivered(id: bytes32) {

}

#[receive(contract = "vendor", name = "seller_ForfeitDispute")]
fn seller_ForfeitDispute(id: bytes32) {

}

#[receive(contract = "vendor", name = "seller_CounterDispute", payable)]
fn seller_CounterDispute(id: bytes32, randomBit: bool) {

}

#[receive(contract = "vendor", name = "seller_UpdateListings")]
fn seller_UpdateListings(itemId: u64, description: String, value: u64) {

}




































#[receive(contract = "auction", name = "bid", payable, mutable)]


#[receive(contract = "auction", name = "finalize", mutable)]



#[concordium_cfg_test]
mod tests {
    use std::borrow::{Borrow, BorrowMut};
    use std::cmp::min;
    use std::ops::Range;
    use super::*;
    use concordium_std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::thread::{current, sleep};
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



    ///QUICKCHECK

    use quickcheck::{Gen, Arbitrary, Testable};
    use quickcheck_macros::quickcheck;
    use rand::{Rng, SeedableRng, thread_rng};
    use crate::schema::SizeLength::U64;


}