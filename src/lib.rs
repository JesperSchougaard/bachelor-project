#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(warnings, unused)]

use std::collections::{BTreeMap, HashMap};
extern crate core;
extern crate crypto;
//use self::crypto::digest::Digest;
//use self::crypto::sha3::Sha3;

use concordium_std::*;
use core::fmt::Debug;
use std::borrow::{Borrow, BorrowMut};
use std::fmt::Error;
use std::io::Bytes;
use std::num::NonZeroI32;
use bytes32::Bytes32;
use crate::Address::Account;
//use concordium_std::schema::Type::Timestamp;
use concordium_std::Timestamp;
use serde::ser::StdError;
use crate::Errors::{MatchingAccountError, MatchingItemValueError, ProgrammerError};
use crate::schema::Type::I32;
use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Rejected, Requested};

#[derive(Serialize, SchemaType)]
struct InitParameter {
    // address of whoever is the "vendor" (should be seller)
    vendor: AccountAddress,
    // length of time the smart contract runs for
    timeout: u64,
}

//impl fmt::Display for State {
//    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//        write!(f, "{:?}", self)
//    }
//}
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

#[derive(Serialize, SchemaType, Clone)]
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
    return Timestamp::from_timestamp_millis(t.timestamp_millis() + 1)
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
        listings: vec!(Item { item_value: 0, description: "".to_string() }),
    };
    Ok(state)
}

// BUYER  FUNCTIONS

#[derive(Serialize, SchemaType)]
struct buyer_RequestPurchaseParameter {
    info: String ,
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
    println!("request purchase amount: {:?}", amount.micro_ccd);
    println!("item value: {:?}", host.state().listings.get(item).unwrap().item_value);

    //ensure!(amount.micro_ccd == host.state().listings.get(item).unwrap().item_value, concordium_std::Reject {error_code: new_unchecked(2), return_value: None});
    ensure!(amount.micro_ccd == host.state().listings.get(item).unwrap().item_value);

    println!("Passed ensure in request purchase");

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


#[receive(contract = "vendor", name = "buyer_Abort", parameter="idParameter", mutable)]
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
    let mut item_value = borrowed_host.listings.get(item).unwrap().clone().item_value;
    let buyer = contract.buyer;
    let sender = ctx.sender();

    //only the buyer can abort the contract
    ensure!(sender.matches_account(&buyer), Errors::MatchingAccountError);

    //Can only abort contract before vendor has interacted with contract
    ensure!(contract.state == State::Requested, Errors::StateError);

    contract.state = Failed;
    //let listings: Vec<Item> = borrowed_host.listings;
    let amount = Amount { micro_ccd: item_value };
    let transfer = host.invoke_transfer(&buyer, amount); // Return money to buyer
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

    contract.state = Failed;
    // Transfer funds to buyer
    host.invoke_transfer(&buyer, Amount { micro_ccd: amount });
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

    let amount_to_transfer = Amount { micro_ccd: 2*item_value };
    if contract.seller_bit != buyerBit {
        host.invoke_transfer(&vendor,  amount_to_transfer);
    } else {
        host.invoke_transfer(&buyer, amount_to_transfer);
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
    host.invoke_transfer(&buyer,  amount);
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

    ensure!(sender.matches_account(&vendor), Errors::MatchingAccountError);//"Only seller can accept the contract");
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
    println!("passed item was delivered ensures");

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
    host.invoke_transfer(&buyer, amount);
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

    ensure!(ctx.sender().matches_account(&host.state().vendor), Errors::MatchingAccountError); // "Only seller can update listings"

    let item =  host.state_mut().listings.get_mut(parameters.itemId as usize).unwrap();
    item.item_value = parameters.value * (10^9); // Multiply by 10^9 to get gwei in wei
    item.description = parameters.description;
    Ok(())
}



#[concordium_cfg_test]
mod tests {
    use std::borrow::{Borrow, BorrowMut};
    use std::cmp::min;
    use std::ops::Range;
    use super::*;
    use concordium_std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::thread::{current, sleep};
    use std::{assert_eq, process, time};
    use quickcheck::{Arbitrary, Gen};
    use test_infrastructure::*;
    use quickcheck_macros;

    // A counter for generating new account addresses
    static ADDRESS_COUNTER: AtomicU8 = AtomicU8::new(0);
    const AUCTION_END: u64 = 1;
    const ITEM: &str = "Starry night by Van Gogh";

    fn expect_error<E, T>(expr: Result<T, E>, err: E, msg: &str)
        where
            E: Eq + Debug,
            T: Debug, {
        let actual = expr.expect_err(msg);
        std::assert_eq!(actual, err);
    }

    fn create_parameter_bytes(parameter: &InitParameter) -> Vec<u8> { to_bytes(parameter) }

    fn parametrized_init_ctx(parameter_bytes: &Vec<u8>) -> TestInitContext {
        let mut ctx = TestInitContext::empty();
        ctx.set_parameter(parameter_bytes);
        ctx
    }

    //fn createInitialState() -> ContractState {
    //    let init_parameter = init_create_parameter();
    //    let parameter_bytes = create_parameter_bytes(&init_parameter);
    //    let ctx0 = parametrized_init_ctx(&parameter_bytes);
    //    let initial_state = commerce_init(&ctx0, &mut TestStateBuilder::new()).expect("Initialization should pass");
    //    return initial_state;
    //}

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

    //QUICKCHECK

    //use quickcheck::{Gen, Arbitrary, Testable};
    use quickcheck_macros::quickcheck;
    //use rand::{Rng, SeedableRng, thread_rng};
    //use crate::schema::SizeLength::U64;

    use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Null, Rejected, Requested};


    #[derive(Clone, Debug)]
    pub struct ValidPath(Vec<Choice>);

    impl fmt::Display for ValidPath {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            self.0.iter().fold(Ok(()), |result, choice| {
                result.and_then(|_| writeln!(f, "{}, ", choice))
            })
        }
    }

    impl fmt::Display for Choice {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Choice(State: {:?})", self.state)
        }
    }

    #[derive(Clone, Debug, Copy)]
    pub struct Choice{ state: State, func: Funcs }

    //clone values
    /*impl Clone for ValidPath {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }*/

    #[derive(Clone, Debug, Copy)]
    pub enum Funcs {
        buyer_Abort,
        seller_RejectContract,
        seller_ItemWasDelivered,
        buyer_ConfirmDelivery,
        seller_CallTimeout,
        buyer_DisputeDelivery,
        seller_ForfeitDispute,
        seller_CounterDispute,
        buyer_OpenCommitment,
        buyer_CallTimeout,
    }

    //make arbitrary values
    impl Arbitrary for ValidPath {
        fn arbitrary(g: &mut Gen) -> Self {

            let stateMappings: BTreeMap<State, Vec<Choice>> = BTreeMap::from([
                (Requested, vec![
                    Choice { state: Accepted, func: Funcs::seller_ItemWasDelivered},
                    Choice { state: Failed, func: Funcs::buyer_Abort},
                    Choice { state: Rejected, func: Funcs::seller_RejectContract}
                ]),
                (Rejected, vec![]), //Empty means we should be done
                (Accepted, vec![
                    Choice { state: Failed, func: Funcs::buyer_CallTimeout},
                    Choice { state: Delivered, func: Funcs::seller_ItemWasDelivered}
                ]),
                (Delivered, vec![
                    Choice { state: Completed, func: Funcs::buyer_ConfirmDelivery},
                    Choice { state: Completed, func: Funcs::seller_CallTimeout},
                    Choice { state: Dispute, func: Funcs::buyer_DisputeDelivery}
                ]),
                (Dispute, vec![
                    Choice { state: Failed, func: Funcs::buyer_CallTimeout},
                    Choice { state: Failed, func: Funcs::seller_ForfeitDispute},
                    Choice { state: Counter, func: Funcs::seller_CounterDispute}
                ]),
                (Counter, vec![
                    Choice { state: Failed, func: Funcs::buyer_OpenCommitment},
                    Choice { state: Completed, func: Funcs::seller_CallTimeout}
                ]),
                (Completed, vec![]),
                (Failed, vec![]),
            ]);

            // Start at state Requested
            let mut choices: &Vec<Choice> = stateMappings.get(&State::Requested).unwrap();

            // then we add that our path started in state Requested
            let mut path: Vec<Choice> = vec![];

            // Now we take random decisions from the ones possible
            while choices.len() > 0 {
                let randomIndex = (u64::arbitrary(g) as usize) % choices.len();
                let &Choice {state, func} = choices.get(randomIndex).unwrap();
                println!("Pushing choice: {:?}", Choice {state, func});
                path.push(Choice {state, func});
                choices = stateMappings.get(&state).unwrap();
            }

            return ValidPath(path);
        }
    }


    fn create_ctx_with_owner<'a>(
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

    fn create_ctx<'a>(
        sender: AccountAddress,
        slot_time: u64,
    ) -> TestReceiveContext<'a> {
        let mut ctx = TestReceiveContext::empty();
        ctx.set_sender(Address::Account(sender));
        ctx.set_metadata_slot_time(Timestamp::from_timestamp_millis(slot_time));
        ctx
    }

    #[quickcheck]
    fn property_check(validPath: ValidPath) -> bool {
        println!("================================================================================================================================================================================================================================================================================================================================================================================================");

        // Create a test statebuilder
        let mut state_builder = TestStateBuilder::new();

        // Create owner, aka vendor, addresses and contexts
        let owner_accountAddress = new_account();
        let owner_account = Address::Account(owner_accountAddress);
        let owner_ctx = create_ctx_with_owner(
            owner_accountAddress,
            owner_accountAddress,
            2
        );

        // Create buyer addresses and contexts
        let buyer_accountAddress = new_account();
        let buyer_account = Address::Account(buyer_accountAddress);
        let buyer_ctx = create_ctx(buyer_accountAddress, 3);

        // Create smart contract initialization parameter
        let init_parameter = create_parameter_bytes(&InitParameter {
            vendor: owner_accountAddress,
            timeout: 3
        });

        // Get initial contract state by initializing the contract
        let initialContractState: ContractState = commerce_init(
            &parametrized_init_ctx(&init_parameter),
            &mut state_builder
        ).expect("Initialization should pass");

        // Create a testhost with initial contract state and the test statebuilder
        //let &mut host: &mut TestHost<ContractState>   = TestHost::new(initialContractState.clone(), state_builder);
        let mut host = TestHost::new(initialContractState.clone(), state_builder);

        // Create parameter for requesting a purchase
        let (buyer_RequestPurchaseParameter) = to_bytes(&buyer_RequestPurchaseParameter{
            info: "".to_string(),
            timestamp: 0,
            item: 0,
        });

        // Get the contract state from the host, then insert an item
        let state = host.state_mut();
        state.listings.insert(0, Item { item_value: 21, description: "Some item".to_string() });

        //println!("[A1] Contracts: {:?}", state.contracts.clone());
        println!("[A1] Contracts: {:?}______________________________", host.state_mut().contracts);

        // Create the id by requesting a purchase
        let id = buyer_RequestPurchase(
            create_ctx(buyer_accountAddress, 3).set_parameter(&buyer_RequestPurchaseParameter),
            &mut host,
            Amount {micro_ccd: 21}
        ).unwrap();

        //println!("[A2] Contracts: {:?}", state.contracts.clone());
        println!("[A2] Contracts: {:?}______________________________", host.state_mut().contracts);

        // buyer_RequestPurchase(&ctx0, &mut host, Amount { micro_ccd: 10 });

        let contract_name =  "Commerce Contract";
        let id_parameter = to_bytes(&idParameter{
            id: id,
        });
        let seller_id_randomBit_param = to_bytes(&seller_CounterDispute {
            id: id,
            randomBit: false, // type bool
        });
        let buyer_id_commitment_parameter = to_bytes(&Buyer_id_commitment {
            id: id,
            commitment: 1, // dummy
        });
        let buyer_id_buyerBit_nonce_param =  to_bytes(&buyer_OpenCommitmentParameter {
            id: id,
            buyerBit: true,
            nonce: 0, // dummy
        });

        let test_param = to_bytes(&seller_UpdateListings {
            itemId: 0,
            description: "".to_string(),
            value: 0
        });
        let mut test_ctx = parametrized_init_ctx(&test_param);

        println!("\n\nValidPath to test: [{}]; \n\n", validPath);

        //let mut currentContractState = host.state_mut();
        //println!("[1s3] contract state: {:?}", currentContractState.contracts.get(0));
        //let mut currentContractState = host.state_mut().clone();


        for choice in validPath.0.into_iter() {

            //println!("[2s3] state: {:?}--------------------------", host.state_mut().contracts.get(0));
            match choice.func {
                Funcs::seller_ItemWasDelivered => seller_ItemWasDelivered(create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::buyer_Abort => buyer_Abort(create_ctx(buyer_accountAddress, 3).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::seller_RejectContract => seller_RejectContract(create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::buyer_CallTimeout => buyer_CallTimeout( create_ctx(buyer_accountAddress, 3).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::seller_ItemWasDelivered => seller_ItemWasDelivered(create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::buyer_ConfirmDelivery => buyer_ConfirmDelivery(create_ctx(buyer_accountAddress, 3).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::seller_CallTimeout => seller_CallTimeout(create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::buyer_DisputeDelivery => buyer_DisputeDelivery(create_ctx(buyer_accountAddress, 3).set_parameter(&buyer_id_commitment_parameter), host.borrow_mut(), Amount {micro_ccd: 21}),
                Funcs::buyer_CallTimeout => buyer_CallTimeout(create_ctx(buyer_accountAddress, 3).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::seller_ForfeitDispute => seller_ForfeitDispute( create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::seller_CounterDispute => seller_CounterDispute( create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&seller_id_randomBit_param), host.borrow_mut(), Amount{ micro_ccd: 21}),
                Funcs::buyer_OpenCommitment => buyer_OpenCommitment(create_ctx(buyer_accountAddress, 3).set_parameter(&id_parameter), host.borrow_mut()),
                Funcs::seller_CallTimeout => seller_CallTimeout( create_ctx_with_owner(owner_accountAddress, owner_accountAddress, 2).set_parameter(&id_parameter), host.borrow_mut()),
                _ => {println!("WE SHOULD NEVER GET HERE"); return false;}
            };


            println!("[s1] choice: {}", choice);
            println!("[s2] choice func: {:?}", choice.func);
            println!("[s3] state: {:?}--------------------------", host.state_mut().contracts.get(0).unwrap().state);
            //assert!(host.state_mut().contracts.get(0).unwrap().state == choice.state); // THIS IS THE PROPERTY WE ARE TESTING

            //let contractState: &mut ContractState = host.state_mut(); // get new ContractState
            //currentContractState = contractState; // update current ContractState for next iteration
        }

        //Things to consider:

        //if reaching Rejected, requested must be the last state visited
        //if reaching Accepted, requested must be the last state visited

        //if reaching Failed, last state MUST be Requested OR Accepted OR Counter OR Dispute

        //if reaching Rejected OR Failed OR Completed, we are done with the trade

        //Should ensure accounts get their money back if something goes wrong
        //Should ensure that seller gets paid if we get to the Completed State

        return true

    }


}