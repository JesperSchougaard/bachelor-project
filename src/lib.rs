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
use bytes32::Bytes32;
use crate::Address::Account;
//use concordium_std::schema::Type::Timestamp;
use concordium_std::Timestamp;
use serde::ser::StdError;
use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Rejected, Requested};

#[derive(Serialize, SchemaType)]
struct InitParameter {
    // address of whoever is the "vendor" (should be seller)
    vendor: AccountAddress,
    // length of time the smart contract runs for
    timeout: u64,
}

/// `concordium-client contract show`.
#[derive(Serialize, Debug)]
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

#[derive(Serialize, SchemaType)]
struct Item {
    item_value: u64,
    description: String,
}

/// A single purchase by a buyer.
#[derive(Serialize)]
struct Purchase {
    commit: u64,        // Commitment to buyer random bit
    timestamp: Timestamp,    // The last block where activity was recorded (for timeouts).
    item: u64,               // Identifier of the item purchased.
    seller_bit: bool,        // Seller random bit

    notes: String,           // Buyer notes about purchase (shipping etc.)
    state: State,            // Current state of the purchase.

    buyer: AccountAddress,   // Address of the buyer
}

#[derive(Serialize)]
pub struct ContractState {
    vendor: AccountAddress,
    timeout: u64,
    state: State,
    contracts: Vec<Purchase>,
    listings: Vec<Item>,
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
        state: State::Null,
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

#[receive(contract = "vendor", name = "buyer_RequestPurchase", parameter = "buyer_RequestPurchaseParameter", payable)]
fn buyer_RequestPurchase<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType=S>,
    amount: Amount,
) -> InitResult<String> {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let info = parameters.info;
    let timestamp = parameters.timestamp;
    let item: usize = parameters.item as usize;

    //let host.state().listings;
    ensure!(amount.micro_ccd == host.state().listings.get(item).unwrap().item_value); //must pay correct amount
    //let mut hasher = Sha3::keccak256();

    //let str = Timestamp::from_timestamp_millis(timestamp).timestamp_millis().to_string();// + ctx.sender()
    //hasher.input_str("placeholder");
    let id: String = "placeholder".to_string();

    host.state().contracts.push(Purchase {
        commit: 0,
        timestamp: Timestamp::from_timestamp_millis(timestamp),
        item: item,
        seller_bit: false,
        notes: info,
        state: Requested,
        buyer: ctx.sender.address,
    });
    Ok(id.parse().unwrap())
}


#[derive(SchemaType)]
struct idParameter {
    id: u64
}

#[derive(SchemaType)]
struct buy_abortParameter {
    id: u64,
    item: u64
}

#[receive(contract = "vendor", name = "buyer_Abort", parameter="idParameter")]
fn buyer_Abort<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let item: usize = parameters.item as usize;

    let contract: StateRef<Purchase> = *host.state().contracts.get(id).unwrap();
    // Only the buyer can abort the contract.
    let buyer = contract.buyer;
    let sender = ctx.sender();
    let state = contract.state.copy();
    ensure!(sender.matches_account(&buyer), "only the buyer can abort the contract");
    ensure!(*state == State::Requested,
        "Can only abort contract before vendor has interacted with contract");

    contract.state = Failed;
    // contracts[id].buyer.transfer(address(this).balance);
    let amount = Amount { micro_ccd: host.state().listings.get(item).unwrap().item_value };
    host.invoke_transfer((&contract.buyer), amount); // Return money to buyer
    // TODO: correct?

}


#[receive(contract = "vendor", name = "buyer_ConfirmDelivery", parameter="idParameter")]
fn buyer_ConfirmDelivery<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;

    let contract: StateRef<Purchase> = *host.state().contracts.get(id).unwrap();
    let buyer = contract.buyer;
    let sender = ctx.sender();
    ensure!(sender.matches_account(&buyer), "Only buyer can confirm the delivery");
    ensure!(contract.state == Delivered, "Can only confirm after vendor has claimed delivery");

    contract.state = Completed;
    // send payment to seller  (corresponding to the price of the item)
    let amount = host.state().listings.get(&contract.item).item_value;
    host.invoke_transfer(&host.state().vendor, amount);

}


#[derive(SchemaType)]
struct Buyer_id_commitment {
    id: u64,
    commitment: u64,
}

#[receive(contract = "vendor", name = "buyer_DisputeDelivery", parameter = "buyer_id_commitment", payable)]
fn buyer_DisputeDelivery<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
    amount: Amount,
) {
    let parameters: Buyer_id_commitment = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let commitment = parameters.commitment;

    let contract = host.state().contracts.get(&id).unwrap();
    let buyer = contract.buyer;
    let sender = ctx.sender();
    ensure!(sender.matches_account(&buyer), "Only buyer can dispute the delivery");
    ensure!(contract.state == Delivered, "Can only dispute delivery when vendor has claimed delivery");
    ensure!(host.listings.get(contract.item).item_value == amount, "Has to wager same value as transaction");
    // ContractState.listings.entry(item).item_value

    contract.state = Dispute;
    contract.commit = commitment; // Store buyer's commitment to random bit
    contract.timestamp = tsfix(contract.timestamp);
}


/// @notice Buyer calls timeout and receives back the money in the contract. Can only be done if timeout seconds has passed without seller action.
/// @param id Hash of the contract.
#[receive(contract = "vendor", name = "buyer_CallTimeout", parameter="idParameter")]
fn buyer_CallTimeout<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: Buyer_id_commitment = ctx.parameter_cursor().get()?;
    let id = parameters.id;

    let contract: StateRef<Purchase> = *host.state().contracts.get(&id).unwrap();
    let buyer = contract.buyer;
    let sender = ctx.sender();
    ensure!(sender.matches_account(buyer), "Only buyer can call this timeout function");
    ensure!(contract.state == Dispute || contract.state == Accepted,
        "contract state is not disputed or accepted");
    ensure!(tsfix(contract.timestamp) > contract.timestamp + host.state().timeout,
        "can only call timeout when timeout seconds has passed");

    contract.state = Failed;
    // Transfer funds to buyer

    let amount = host.state().listings.get(&contract.item).item_value;
    host.invoke_transfer(&contract.buyer, amount);
}


#[derive(SchemaType)]
struct buyer_OpenCommitmentParameter {
    id: u64,
    buyerBit: bool,
    nonce: u64
}
#[receive(contract = "vendor", name = "buyer_OpenCommitment", parameter="buyer_OpenCommitmentParameter")]
fn buyer_OpenCommitment<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: buyer_OpenCommitmentParameter = ctx.parameter_cursor.get?;
    let id = parameters.id;
    let buyerBit = parameters.buyerBit;
    let nonce = parameters.nonce;

    let contract: StateRef<Purchase> = *host.state().contracts.get(&id).unwrap();
    let buyer = contract.buyer;
    let sender = ctx.sender();
    ensure!(sender.matches_account(&buyer), "Only buyer can open commitment");
    ensure!(contract.state == Counter, "Can only open commitment if seller has countered.");

    //let mut hasher = Sha3::keccak256();
    //hasher.input_str("placeholder"); // probably not what we are looking for &(stringify!(buyerBit, id, nonce))
    //let hashed: String = hasher.result_str();

    ensure!(contract.commit == 0, "Check that (_buyerBit,id,nonce) is opening of commitment");

    contract.state = Failed;
    let value = host.state().listings.get(&contract.item).unwrap().item_value;
    let amount_to_transfer = Amount { micro_ccd: 2*value };
    if contract.seller_bit != buyerBit {
        host.invoke_transfer(&host.state().vendor,  amount_to_transfer);
    } else {
        host.invoke_transfer(&contract.buyer, amount_to_transfer);
    }
}

// SELLER FUNCTIONS

#[receive(contract = "vendor", name = "seller_CallTimeout", parameter="idParameter")]
fn seller_CallTimeout<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract = host.state().contracts.get(&id).unwrap();
    let vendor = host.state().vendor;
    let sender = ctx.sender();
    ensure!(sender.matches_account(&vendor), "Only seller can call this timeout function");
    ensure!(contract.state == Delivered
        || contract.state == Counter,
        "The buyer has either not responded to delivery OR the buyer does not open their commitment");
    ensure!(tsfix(contract.timestamp) > contract.timestamp + host.state().timeout,
        "Can only timeout after timeout second");

    contract.state = Completed;
    // Transfer funds to seller
    let amount = host.state().listings.get(&contract.item).item_value;
    host.invoke_transfer(&host.state().vendor, amount);
}


#[receive(contract = "vendor", name = "seller_RejectContract", parameter = "idParameter")]
fn seller_RejectContract<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract = host.state().contracts.get(&id).unwrap();

    ensure!(ctx.sender().matches_account(host.state().vendor), "only seller can reject the contract");
    ensure!(contract.state == Requested, "can only reject contract when buyer has requested");

    contract.state = Rejected;
    // transfer funds back to buyer
    let amount = Amount { micro_ccd: host.state().listings.get(&contract.item).unwrap().item_value};
    host.invoke_transfer(&contract.buyer, amount);
}


#[receive(contract = "vendor", name = "seller_AcceptContract", parameter = "idParameter")]
fn seller_AcceptContract<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract: StateRef<Purchase> = *host.state().contracts.get(&id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "Only seller can accept the contract");
    ensure!(contract.state == Requested, "Can only accept contract when buyer has requested");

    contract.state = Accepted;
    contract.timestamp = tsfix(contract.timestamp);
}


#[receive(contract = "vendor", name = "seller_ItemWasDelivered", parameter = "idParameter")]
fn seller_ItemWasDelivered<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;

    let contract = host.state().contracts.get(&id).unwrap();
    ensure!(ctx.sender() == host.state().vendor);
    ensure!(contract.state == Accepted);

    contract.state = Delivered;
    contract.timestamp = tsfix(contract.timestamp);
}

#[receive(contract = "vendor", name = "seller_ForfeitDispute", parameter = "idParameter")]
fn seller_ForfeitDispute<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract = host.state().contracts.get(&id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "Only seller can forfeit the dispute of the buyer");
    ensure!(contract.state == Dispute, "Can only forfeit dispute if buyer disputed delivery");

    contract.state = Failed;
    // Transfer funds to buyer
    let amount = Amount { micro_ccd: host.state().listings.get(&contract.item).unwrap().item_value };
    host.invoke_transfer(&contract.buyer, amount);
}


#[derive(SchemaType)]
struct seller_CounterDispute {
    id: u64,
    randomBit: bool,
}
#[receive(contract = "vendor", name = "seller_CounterDispute", parameter="seller_CounterDispute", payable)]
fn seller_CounterDispute<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
    amount: Amount,
) {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let randomBit = parameters.randomBit;

    let contract = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "only seller can counter dispute");
    ensure!(contract.state == Dispute, "can only counter disputre if buyer disputed delivery");
    let listing_amount = Amount { micro_ccd: host.state().listings.get(&contract.item).unwrap().item_value};
    ensure!(amount == listing_amount, "seller has to wager the value of the item");

    contract.state = Counter;
    contract.timestamp = tsfix(contract.timestamp);
    contract.seller_bit = randomBit;
}

#[derive(SchemaType)]
struct seller_UpdateListings {
    itemId: u64,
    description: String,
    value: u64,
}
#[receive(contract = "vendor", name = "seller_UpdateListings", parameter = "seller_UpdateListings")]
fn seller_UpdateListings<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState, StateApiType = S>,
) {
    let parameters: seller_UpdateListings = ctx.parameter_cursor().get()?;

    ensure!(ctx.sender() == host.state().vendor, "Only seller can update listings");

    let item =  host.state().listings.get(&parameters.itemId).unwrap();
    item.item_value = parameters.value * (10**9); // Multiply by 10^9 to get gwei in wei
    item.description = parameters.description;
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

    fn init_create_parameter() -> InitParameter {
        InitParameter {
            vendor: new_account(),
            timeout: 3
        }
    }

    fn create_parameter_bytes(parameter: &InitParameter) -> Vec<u8> { to_bytes(parameter) }

    fn parametrized_init_ctx(parameter_bytes: &Vec<u8>) -> TestInitContext {
        let mut ctx = TestInitContext::empty();
        ctx.set_parameter(parameter_bytes);
        ctx
    }

    fn createInitialState<'a, S: HasStateApi>() -> ContractState {
        let init_parameter = init_create_parameter();
        let parameter_bytes = create_parameter_bytes(&init_parameter);
        let ctx0 = parametrized_init_ctx(&parameter_bytes);
        let initial_state = commerce_init(&ctx0, &mut TestStateBuilder::new()).expect("Initialization should pass");
        return initial_state;
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

    //QUICKCHECK

    //use quickcheck::{Gen, Arbitrary, Testable};
    use quickcheck_macros::quickcheck;
    //use rand::{Rng, SeedableRng, thread_rng};
    //use crate::schema::SizeLength::U64;

    use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Null, Rejected, Requested};


    //#[derive(Sized)]
    pub struct ValidPath(Vec<Choice>);

    pub struct Choice{ state: State, func: Funcs }

    //clone values
    impl Clone for ValidPath {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

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
            let mut choices: &Vec<Choice> = stateMappings.get(&State::Null).unwrap();

            // then we add that our path started in state Requested
            let mut path: Vec<Choice> = vec![*choices.get(0).unwrap()];

            // Now we take random decisions from the ones possible
            while choices.len() > 0 {
                let randomIndex = u64::arbitrary(g) % choices.len();
                let &Choice {state, func} = choices.get(randomIndex).unwrap();
                path.push(Choice {state, func});
                choices = stateMappings.get(&state).unwrap();
            }

            return ValidPath(path);
        }
    }

    #[quickcheck]
    fn property_check(validPath: ValidPath) -> bool {

        let initialContractState: ContractState<dyn HasStateApi> = createInitialState();
        //let host: &mut TestHost<ContractState<dyn HasStateApi>> = &mut TestHost::new(currentContractState.cloned().state, TestStateBuilder::new());


        let init_parameter = init_create_parameter();
        let parameter_bytes = create_parameter_bytes(&init_parameter);
        let ctx0 = parametrized_init_ctx(&parameter_bytes);
        let &mut state_builder = TestStateBuilder::new();
        let initial_state = commerce_init(&ctx0, state_builder).expect("Initialization should pass");
        let mut host = TestHost::new(initial_state, state_builder);

        let buyer_RequestPurchaseParameter = to_bytes(&buyer_RequestPurchaseParameter{
            info: "".to_string(),
            timestamp: 0,
            item: 0,
        });
        let ctx_buyerRequestPurchase_param = TestInitContext::empty().set_parameter(buyer_RequestPurchaseParameter);
        let id = buyer_RequestPurchase(ctx_buyerRequestPurchase_param, &mut host, Amount {micro_ccd: 21}).unwrap();


        // buyer_RequestPurchase(&ctx0, &mut host, Amount { micro_ccd: 10 });

        let contract_name =  "Commerce Contract";
        let id_parameter = to_bytes(&idParameter{
            id: to_bytes(&id.clone()),
        });
        let seller_id_randomBit_param = to_bytes(&seller_CounterDispute {
            id: to_bytes(&id.clone()),
            randomBit: false, // type bool
        });
        let buyer_id_commitment_parameter = to_bytes(&Buyer_id_commitment {
            id: to_bytes(&id.clone()),
            commitment: to_bytes(&id.clone()),
        });
        let buyer_id_buyerBit_nonce_param =  to_bytes(&buyer_OpenCommitmentParameter {
            id: to_bytes(&id.clone()),
            buyerBit: true,
            nonce: to_bytes(&id.clone()),
        });

        let ctx_idParameter = TestInitContext::empty().set_parameter(id_parameter);
        let ctx_id_randomBit_param = TestInitContext::empty().set_parameter(seller_id_randomBit_param);
        let ctx_id_commit_param = TestInitContext::empty().set_parameter(buyer_id_commitment_parameter);
        let ctx_id_buyerBit_nonce_param = TestInitContext::empty().set_parameter(buyer_id_buyerBit_nonce_param);

        let test_param = to_bytes(&seller_UpdateListings {
            itemId: 0,
            description: "".to_string(),
            value: 0
        });
        let mut test_ctx = parametrized_init_ctx(test_param);


        let mut currentContractState = initialContractState;
        for choice in validPath.0.into_iter() {
            assert!(currentContractState.state == choice.state); // THIS IS THE PROPERTY WE ARE TESTING


            match choice.clone().get(choice.func) {
                "seller_ItemWasDelivered" => seller_ItemWasDelivered(ctx_idParameter, &mut host),
                "buyer_Abort" => buyer_Abort(ctx_idParameter, &mut host),
                "seller_RejectContract" => seller_RejectContract(ctx_idParameter, &mut host),
                "buyer_CallTimeout" => buyer_CallTimeout( ctx_idParameter, &mut host),
                "seller_ItemWasDelivered" => seller_ItemWasDelivered( ctx_idParameter, &mut host),
                "buyer_ConfirmDelivery" => buyer_ConfirmDelivery( ctx_idParameter, &mut host),
                "seller_CallTimeout" => seller_CallTimeout( ctx_idParameter, &mut host),
                "buyer_DisputeDelivery" => buyer_DisputeDelivery(buyer_id_commitment_parameter, &mut host, Amount {micro_ccd: 21}),
                "buyer_CallTimeout" => buyer_CallTimeout( ctx_idParameter, &mut host),
                "seller_ForfeitDispute" => seller_ForfeitDispute( ctx_idParameter, &mut host),
                "seller_CounterDispute" => seller_CounterDispute( ctx_id_randomBit_param, &mut host, Amount{ micro_ccd: 21}),
                "buyer_OpenCommitment" => buyer_OpenCommitment(ctx_idParameter, &mut host),
                "seller_CallTimeout" => seller_CallTimeout(ctx_idParameter, &mut host),
                _ => bail!("We shouldn't get here"),
            }



            let contractState: ContractState<dyn HasStateApi> = host.state_mut().clone(); // get new ContractState
            currentContractState = contractState; // update current ContractState for next iteration
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