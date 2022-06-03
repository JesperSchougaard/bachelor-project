#![allow(non_snake_case, non_camel_case_types, unused_doc_comments, dead_code, unused_variables, unused_must_use, unused_attributes)]


use concordium_std::*;
use core::fmt::Debug;
use std::borrow::BorrowMut;
use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Rejected, Requested};

fn main(){}

#[derive(Serialize, SchemaType)]
struct InitParameter {
    // address of whoever is the "vendor" (should be seller)
    vendor: AccountAddress,

    // length of time the smart contract runs for
    timeout: u64,
}

/// `concordium-client contract show`.
#[derive(Serialize, PartialEq, Eq, Debug, Clone, PartialOrd, Ord, Copy)]
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

#[derive(Serialize, SchemaType, Clone, Debug)]
struct Item {
    item_value: u64,
    description: String,
}

/// A single purchase by a buyer.
#[derive(Serialize, Clone, Debug)]
struct Purchase {
    commit: u64,        // Commitment to buyer random bit
    timestamp: Timestamp,    // The last block where activity was recorded (for timeouts).
    item: u64,               // Identifier of the item purchased.
    seller_bit: bool,        // Seller random bit

    notes: String,           // Buyer notes about purchase (shipping etc.)
    state: State,            // Current state of the purchase.

    buyer: AccountAddress,   // Address of the buyer
}

#[derive(Serialize, Clone)]
pub struct ContractState {
    vendor: AccountAddress,
    timeout: u64,
    contracts: Vec<Purchase>,
    listings: Vec<Item>,
}

#[derive(Debug, PartialEq, Eq, Reject)]
enum Errors {
    ParameterError,
    TransferError,
    ParseError,
    MatchingAccountError,
    StateError,
    MatchingItemError,
    MatchingItemValueError,
    TimestampError,
    CommitError,
    ProgrammerError,
    AmountError,
}

fn tsfix(t: Timestamp) -> Timestamp {
    return Timestamp::from_timestamp_millis(t.timestamp_millis() + 10)
}

#[init(contract = "vendor", parameter = "InitParameter")]
fn commerce_init<S: HasStateApi>(
    ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<ContractState> {
    let parameter: InitParameter = ctx.parameter_cursor().get()?;
    ensure!(parameter.timeout > 0);
    let state = ContractState {
        vendor: parameter.vendor,
        timeout: parameter.timeout,
        contracts: vec!(),
        listings: vec!(),
    };
    Ok(state)
}

// BUYER  FUNCTIONS

#[derive(Serialize, SchemaType)]
struct buyer_RequestPurchaseParameter {
    info: String,
    timestamp: u64,
    item: u64,
}

#[receive(contract = "vendor", name = "buyer_RequestPurchase", parameter = "buyer_RequestPurchaseParameter", payable, mutable)]
fn buyer_RequestPurchase<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType=S>,
    amount: Amount,
) -> InitResult<u64> {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let info = parameters.info;
    let timestamp = parameters.timestamp;
    let item: usize = parameters.item as usize;


    // Must pay correct amount
    ensure!(amount.micro_ccd == host.state().listings.get(item).unwrap().item_value);

    //println!("Passed ensure in request purchase");

    let id = 0; // dummy
    let sender = match ctx.sender() {
        Address::Account(acc) => acc,
        _ => {println!("WE SHOULD NEVER GET HERE"); bail!()}
    };
    //println!("__{:?}__",sender);

    host.state_mut().contracts.push(Purchase {
        commit: 0,
        timestamp: Timestamp::from_timestamp_millis(timestamp),
        item: 0,
        seller_bit: false,
        notes: info,
        state: Requested,
        buyer: sender,
    });
    Ok(id)
}


#[derive(SchemaType, Serialize)]
struct idParameter {
    id: u64
}

#[derive(SchemaType, Serialize)]
struct buy_abortParameter {
    id: u64,
    item: u64
}


#[receive(contract = "vendor", name = "buyer_Abort", parameter="buy_abortParameter", mutable)]
fn buyer_Abort<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {  // Result<(), FinalizeError>
    let parameters: buy_abortParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let item: usize = parameters.item as usize;
    let borrowed_host = host.state_mut();
    let mut contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item_value = borrowed_host.listings.get(item).unwrap().clone().item_value;
    let buyer = contract.buyer;
    let sender = ctx.sender();

    //only the buyer can abort the contract
    ensure!(sender.matches_account(&buyer), Errors::MatchingAccountError);

    //Can only abort contract before vendor has interacted with contract
    ensure!(contract.state == State::Requested, Errors::StateError);

    contract.state = Failed;
    //let listings: Vec<Item> = borrowed_host.listings;
    let amount = Amount { micro_ccd: item_value };
    let transfer = host.invoke_transfer(&buyer, amount);
    // Return money to buyer
    // TODO: correct?
    Ok(())
}


#[receive(contract = "vendor", name = "buyer_ConfirmDelivery", parameter="idParameter", mutable)]
fn buyer_ConfirmDelivery<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType=S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let contract = host.state_mut().contracts.get_mut(id).unwrap();
    let buyer = contract.buyer;
    let sender = ctx.sender();

    // Only buyer can confirm the delivery
    ensure!(sender.matches_account(&buyer), Errors::MatchingAccountError);

    // Can only confirm after vendor has claimed delivery
    ensure!(contract.state == Delivered, Errors::StateError);

    contract.state = Completed;
    // send payment to seller  (corresponding to the price of the item)
    let item: usize = contract.clone().item as usize;
    let amount: u64 = host.state().listings.get(item).unwrap().clone().item_value;
    host.invoke_transfer(&host.state().vendor, Amount { micro_ccd: amount });
    Ok(())
}


#[derive(SchemaType, Serialize)]
struct Buyer_id_commitment {
    id: u64,
    commitment: u64,
}

#[receive(contract = "vendor", name = "buyer_DisputeDelivery", parameter = "buyer_id_commitment", payable, mutable)]
fn buyer_DisputeDelivery<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
    amount: Amount,
) -> Result<(), Errors> {
    let parameters: Buyer_id_commitment = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let commitment = parameters.commitment;
    let borrowed_host = host.state_mut();
    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item: usize = contract.clone().item as usize;
    let item_value = borrowed_host.listings.get(item).unwrap().clone().item_value;
    let buyer = contract.buyer;
    let sender = ctx.sender();

    // Only buyer can dispute the delivery
    ensure!(sender.matches_account(&buyer), Errors::MatchingAccountError);

    // Can only dispute delivery when vendor has claimed delivery
    ensure!(contract.state == Delivered, Errors::StateError);

    // Has to wager same value as transaction
    ensure!(item_value == amount.micro_ccd, Errors::MatchingItemValueError);

    contract.state = Dispute;
    // Store buyer's commitment to random bit
    contract.commit = commitment;
    contract.timestamp = tsfix(contract.timestamp);
    Ok(())
}


/// @notice Buyer calls timeout and receives back the money in the contract. Can only be done if timeout seconds has passed without seller action.
/// @param id Hash of the contract.
#[receive(contract = "vendor", name = "buyer_CallTimeout", parameter="idParameter", mutable)]
fn buyer_CallTimeout<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let borrowed_host = host.state_mut();
    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item: usize = contract.clone().item as usize;
    let amount = borrowed_host.listings.get(item).unwrap().clone().item_value;
    let buyer = contract.buyer;
    let sender = ctx.sender();

    // Only buyer can call this timeout function
    ensure!(sender.matches_account(&buyer), Errors::MatchingAccountError);

    // contract state is not disputed or accepted
    ensure!(contract.state == Dispute || contract.state == Accepted, Errors::StateError);

    // can only call timeout when timeout seconds has passed
    ensure!(tsfix(contract.timestamp).timestamp_millis() > contract.timestamp.timestamp_millis()
        + borrowed_host.timeout,
        Errors::TimestampError
    );

    /// Fixed bug here
    let mut payback = amount;
    if contract.state == Dispute {
        payback = amount * 2;
    }
    contract.state = Failed;
    // Transfer funds to buyer
    host.invoke_transfer(&buyer, Amount { micro_ccd: payback });

    Ok(())
}


#[derive(SchemaType, Serialize)]
struct buyer_OpenCommitmentParameter {
    id: u64,
    buyerBit: bool,
    nonce: u64
}

#[receive(contract = "vendor", name = "buyer_OpenCommitment", parameter="buyer_OpenCommitmentParameter", mutable)]
fn buyer_OpenCommitment<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: buyer_OpenCommitmentParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let buyerBit = parameters.buyerBit;
    let nonce = parameters.nonce;

    let borrowed_host = host.state_mut();

    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item: usize = contract.clone().item as usize;
    let item_value = borrowed_host.listings.get(item).unwrap().clone().item_value;
    let buyer = contract.buyer;
    let vendor = borrowed_host.vendor;
    let sender = ctx.sender();

    // Only buyer can open commitment
    ensure!(sender.matches_account(&buyer), Errors::StateError);

    // Can only open commitment if seller has countered
    ensure!(contract.state == Counter, Errors::StateError);

    // Check that commit is 0
    ensure!(contract.commit == 0, Errors::CommitError);

    contract.state = Failed;

    let amount_to_transfer = Amount { micro_ccd: 2 * item_value };
    if contract.seller_bit != buyerBit {
        host.invoke_transfer(&vendor,  amount_to_transfer); // Seller wins
    } else {
        host.invoke_transfer(&buyer, amount_to_transfer);  // buyer wins
    }
    Ok(())
}

// SELLER FUNCTIONS

#[receive(contract = "vendor", name = "seller_CallTimeout", parameter="idParameter", mutable)]
fn seller_CallTimeout<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let borrowed_host = host.state_mut();
    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item: usize = contract.clone().item as usize;
    let amount = borrowed_host.listings.get(item).unwrap().item_value;
    let sender = ctx.sender();
    let vendor = borrowed_host.vendor;

    // Only seller can call this timeout function
    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);

    // The buyer has either not responded to delivery OR the buyer does not open their commitment
    ensure!(contract.state == Delivered || contract.state == Counter, Errors::StateError);

    // Can only timeout after timeout second
    ensure!(tsfix(contract.timestamp).timestamp_millis() > contract.timestamp.timestamp_millis() + borrowed_host.timeout, Errors::TimestampError);

    contract.state = Completed;
    // Transfer funds to seller
    host.invoke_transfer(&vendor, Amount {micro_ccd: amount});
    Ok(())
}


#[receive(contract = "vendor", name = "seller_RejectContract", parameter = "idParameter", mutable)]
fn seller_RejectContract<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let borrowed_host = host.state_mut();
    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item: usize = contract.clone().item as usize;
    let amount = Amount { micro_ccd: borrowed_host.listings.get(item).unwrap().item_value};
    let sender = ctx.sender();
    let vendor = borrowed_host.vendor;
    let buyer = contract.buyer;

    // Only seller can reject the contract
    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);

    // Can only reject contract when buyer has requested
    ensure!(contract.state == Requested, Errors::StateError);

    contract.state = Rejected;
    // transfer funds back to buyer
    host.borrow_mut().invoke_transfer(&buyer,  amount);

    Ok(())
}


#[receive(contract = "vendor", name = "seller_AcceptContract", parameter = "idParameter", mutable)]
fn seller_AcceptContract<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;

    let borrowed_host = host.state_mut();

    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let sender = ctx.sender();
    let vendor = borrowed_host.vendor;

    // Only seller can accept the contract
    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);

    ensure!(contract.state == Requested, Errors::StateError);

    contract.state = Accepted;
    contract.timestamp = tsfix(contract.timestamp);
    Ok(())
}


#[receive(contract = "vendor", name = "seller_ItemWasDelivered", parameter = "idParameter", mutable)]
fn seller_ItemWasDelivered<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let borrowed_host = host.state_mut();
    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let sender = ctx.sender();
    let vendor = borrowed_host.vendor;

    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);
    ensure!(contract.state == Accepted, Errors::StateError);

    contract.state = Delivered;
    contract.timestamp = tsfix(contract.timestamp);
    Ok(())
}

#[receive(contract = "vendor", name = "seller_ForfeitDispute", parameter = "idParameter", mutable)]
fn seller_ForfeitDispute<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: idParameter = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;

    let borrowed_host = host.state_mut();

    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let item: usize = contract.clone().item as usize;
    let item_value = borrowed_host.listings.get(item).unwrap().item_value;
    let sender = ctx.sender();
    let vendor = borrowed_host.vendor;
    let amount = Amount { micro_ccd: item_value };
    let buyer = contract.buyer;

    // Only seller can forfeit the dispute of the buyer
    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);

    // Can only forfeit dispute if buyer disputed delivery
    ensure!(contract.state == Dispute, Errors::StateError);

    contract.state = Failed;
    // Transfer funds to buyer
    host.invoke_transfer(&buyer, amount*2); /// Fixed bug here
    Ok(())
}


#[derive(SchemaType, Serialize)]
struct seller_CounterDispute {
    id: u64,
    randomBit: bool,
}
#[receive(contract = "vendor", name = "seller_CounterDispute", parameter="seller_CounterDispute", payable, mutable)]
fn seller_CounterDispute<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
    amount: Amount,
) -> Result<(), Errors> {
    let parameters: seller_CounterDispute = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let id: usize = parameters.id as usize;
    let randomBit = parameters.randomBit;

    let borrowed_host = host.state_mut();

    let contract = borrowed_host.contracts.get_mut(id).unwrap();
    let sender = ctx.sender();
    let vendor = borrowed_host.vendor;

    let item: usize = contract.clone().item as usize;
    let item_value = borrowed_host.listings.get(item).unwrap().item_value;
    let listing_amount = Amount { micro_ccd: item_value };

    // Only seller can counter dispute
    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);

    // Can only counter dispute if buyer disputed delivery
    ensure!(contract.state == Dispute, Errors::StateError);

    // Seller has to wager the value of the item
    ensure!(amount == listing_amount, Errors::MatchingItemValueError);

    contract.state = Counter;
    contract.timestamp = tsfix(contract.timestamp);
    contract.seller_bit = randomBit;
    Ok(())
}

#[derive(SchemaType, Serialize)]
struct seller_UpdateListings {
    itemId: u64,
    description: String,
    value: u64,
}

#[receive(contract = "vendor", name = "seller_UpdateListings", parameter = "seller_UpdateListings", mutable)]
fn seller_UpdateListings<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) -> Result<(), Errors> {
    let parameters: seller_UpdateListings = match ctx.parameter_cursor().get() {
        Ok(p) => p,
        Err(_) => bail!(Errors::ParseError)
    };
    let item: usize = parameters.itemId as usize;

    // Only seller can update listings
    ensure!(ctx.sender().matches_account(&host.state().vendor), Errors::MatchingAccountError);

    /// This ensure is added to fix the bug that allows the seller to change the price of
    /// the item at any given time during the purchase
    // No buyer has requested the item and where the seller confirmed it before
    ensure!(host.state().contracts.len() == 0, Errors::StateError);

    let item = host.state_mut().listings.get_mut(parameters.itemId as usize).unwrap();
    // Multiply by 10^9 to get gwei in wei
    item.item_value = parameters.value * 10_u64.pow(9);
    //println!("testing 123: {}", 1 * 10_u64.pow(9));
    item.description = parameters.description;
    Ok(())
}
