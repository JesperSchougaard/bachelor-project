#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(warnings, unused)]

extern crate core;
extern crate crypto;
//use self::crypto::digest::Digest;
//use self::crypto::sha3::Sha3;

use concordium_std::*;
use core::fmt::Debug;
use std::borrow::Borrow;
use std::fmt::Error;
use std::io::Bytes;
use bytes32::Bytes32;
use crate::Address::Account;
//use concordium_std::schema::Type::Timestamp;
use concordium_std::Timestamp;
use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Rejected, Requested};

#[derive(Serialize, SchemaType)]
struct InitParameter {
    // address of whoever is the "vendor" (should be seller)
    vendor: AccountAddress,
    // length of time the smart contract runs for
    timeout: u64,
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
struct Purchase<'a> {
    commit: Bytes32<'a>,         // Commitment to buyer random bit
    timestamp: Timestamp,    // The last block where activity was recorded (for timeouts).
    item: u64,               // Identifier of the item purchased.
    seller_bit: bool,        // Seller random bit

    notes: String,           // Buyer notes about purchase (shipping etc.)
    state: State,            // Current state of the purchase.

    buyer: AccountAddress,          // Address of the buyer
}

pub struct ContractState<'a, S> {
    vendor: AccountAddress,
    timeout: u64,
    state: State,
    contracts: StateMap<Bytes32<'a>, Purchase<'a>, S>,
    listings: StateMap<u64, Item, S>,
}

fn tsfix(t: Timestamp) -> Timestamp {
    return Timestamp::from_timestamp_millis(concordium_std::schema::Type::Timestamp.timestamp_millis() + 1)
}

#[init(contract = "vendor", parameter = "InitParameter")]
fn commerce_init<'a, S: HasStateApi>(
    ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<ContractState<'a, S>> {
    let parameter: InitParameter = ctx.parameter_cursor().get()?;
    ensure!(parameter.timeout > 0);
    let state = ContractState {
        state: State::Null,
        vendor: Account { adress: parameter.vendor },
        timeout: parameter.timeout,
        contracts: state_builder.new_map(), //HashMap::new(),
        listings: state_builder.new_map(),
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

#[receive(contract = "vendor", name = "Request_purchase", parameter = "buyer_RequestPurchaseParameter", payable)]
fn buyer_RequestPurchase<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType=S>,
    amount: Amount,
) -> Result<String, Error> {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let info = parameters.info;
    let timestamp = parameters.timestamp;
    let item = parameters.item;

    ensure!(amount == host.state().listings.entry(item).item_value); //must pay correct amount
    //let mut hasher = Sha3::keccak256();

    //let str = Timestamp::from_timestamp_millis(timestamp).timestamp_millis().to_string();// + ctx.sender()
    //hasher.input_str("placeholder");
    let id: String = "placeholder".to_string();

    host.state().B.insert(id, Purchase {
        commit: 0x0,
        timestamp: Timestamp::from_timestamp_millis(timestamp),
        item: item,
        seller_bit: false,
        notes: info,
        state: Requested,
        buyer: ctx.sender.address,
    });
    Ok(id.parse().unwrap())
}


#[derive(Serialize, SchemaType)]
struct idParameter<'a> {
    id: Bytes32<'a>
}

#[derive(Serialize, SchemaType)]
struct buy_abortParameter<'a> {
    id: Bytes32<'a>,
    item: u64
}

#[receive(contract = "vendor", name = "buyer_Abort", parameter="idParameter")]
fn buyer_Abort<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let item = parameters.item;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    // Only the buyer can abort the contract.
    ensure!(ctx.sender() == contract.buyer, "only the buyer can abort the contract");
    ensure!(contract.state == Requested,
        "Can only abort contract before vendor has interacted with contract");

    contract.state = Failed;
    // contracts[id].buyer.transfer(address(this).balance);
    let amount = Amount { micro_ccd: host.state().listings.get(&item).unwrap().item_value };
    host.invoke_transfer((contract.buyer), amount); // Return money to buyer
    // TODO: correct?

}


#[receive(contract = "vendor", name = "buyer_ConfirmDelivery", parameter="idParameter")]
fn buyer_ConfirmDelivery<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == contract.buyer, "Only buyer can confirm the delivery");
    ensure!(contract.state == Delivered, "Can only confirm after vendor has claimed delivery");

    contract.state = Completed;
    // send payment to seller  (corresponding to the price of the item)
    let amount = host.state().listings.get(&contract.item).item_value;
    host.invoke_transfer(&host.state().vendor, amount);

}


#[derive(Serialize, SchemaType)]
struct Buyer_id_commitment<'a> {
    id: Bytes32<'a>,
    commitment: Bytes32<'a>,
}

#[receive(contract = "vendor", name = "buyer_DisputeDelivery", parameter = "buyer_id_commitment", payable)]
fn buyer_DisputeDelivery<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
    amount: Amount,
) {
    let parameters: Buyer_id_commitment = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let commitment = parameters.commitment;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == contract.buyer, "Only buyer can dispute the delivery");
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
fn buyer_CallTimeout<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: Buyer_id_commitment = ctx.parameter_cursor().get()?;
    let id = parameters.id;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == contract.buyer, "Only buyer can call this timeout function");
    ensure!(contract.state == Dispute || contract.state == Accepted,
        "contract state is not disputed or accepted");
    ensure!(tsfix(contract.timestamp) > contract.timestamp + host.state().timeout,
        "can only call timeout when timeout seconds has passed");

    contract.state = Failed;
    // Transfer funds to buyer

    let amount = host.state().listings.get(&contract.item).item_value;
    host.invoke_transfer(&contract.buyer, amount);
}



#[derive(Serialize, SchemaType)]
struct buyer_OpenCommitmentParameter<'a> {
    id: Bytes32<'a>,
    buyerBit: bool,
    nonce: Bytes32<'a>,
}
#[receive(contract = "vendor", name = "buyer_OpenCommitment", parameter="buyer_OpenCommitmentParameter")]
fn buyer_OpenCommitment<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: buyer_OpenCommitmentParameter = ctx.parameter_cursor.get?;
    let id = parameters.id;
    let buyerBit = parameters.buyerBit;
    let nonce = parameters.nonce;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == contract.buyer, "Only buyer can open commitment");
    ensure!(contract.state == Counter, "Can only open commitment if seller has countered.");

    //let mut hasher = Sha3::keccak256();
    //hasher.input_str("placeholder"); // probably not what we are looking for &(stringify!(buyerBit, id, nonce))
    //let hashed: String = hasher.result_str();

    ensure!(contract.commit == 0x0, "Check that (_buyerBit,id,nonce) is opening of commitment");

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
fn seller_CallTimeout<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "Only seller can call this timeout function");
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
fn seller_RejectContract<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();

    ensure!(ctx.sender() == host.state().vendor, "only seller can reject the contract");
    ensure!(contract.state == Requested, "can only reject contract when buyer has requested");

    contract.state = Rejected;
    // transfer funds back to buyer
    let amount = Amount { micro_ccd: host.state().listings.get(&contract.item).unwrap().item_value};
    host.invoke_transfer(&contract.buyer, amount);
}


#[receive(contract = "vendor", name = "seller_AcceptContract", parameter = "idParameter")]
fn seller_AcceptContract<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "Only seller can accept the contract");
    ensure!(contract.state == Requested, "Can only accept contract when buyer has requested");

    contract.state = Accepted;
    contract.timestamp = tsfix(contract.timestamp);
}


#[receive(contract = "vendor", name = "seller_ItemWasDelivered", parameter = "idParameter")]
fn seller_ItemWasDelivered<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == host.state().vendor);
    ensure!(contract.state == Accepted);

    contract.state = Delivered;
    contract.timestamp = tsfix(contract.timestamp);
}

#[receive(contract = "vendor", name = "seller_ForfeitDispute", parameter = "idParameter")]
fn seller_ForfeitDispute<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
) {
    let parameters: idParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "Only seller can forfeit the dispute of the buyer");
    ensure!(contract.state == Dispute, "Can only forfeit dispute if buyer disputed delivery");

    contract.state = Failed;
    // Transfer funds to buyer
    let amount = Amount { micro_ccd: host.state().listings.get(&contract.item).unwrap().item_value };
    host.invoke_transfer(&contract.buyer, amount);
}


#[derive(Serialize, SchemaType)]
struct seller_CounterDispute<'a> {
    id: Bytes32<'a>,
    randomBit: bool,
}
#[receive(contract = "vendor", name = "seller_CounterDispute", parameter="seller_CounterDispute", payable)]
fn seller_CounterDispute<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
    amount: Amount,
) {
    let parameters: buyer_RequestPurchaseParameter = ctx.parameter_cursor().get()?;
    let id = parameters.id;
    let randomBit = parameters.randomBit;

    let contract: StateRef<Purchase> = host.state().contracts.get(id).unwrap();
    ensure!(ctx.sender() == host.state().vendor, "only seller can counter dispute");
    ensure!(contract.state == Dispute, "can only counter disputre if buyer disputed delivery");
    let listing_amount = Amount { micro_ccd: host.state().listings.get(&contract.item).unwrap().item_value};
    ensure!(amount == listing_amount, "seller has to wager the value of the item");

    contract.state = Counter;
    contract.timestamp = tsfix(contract.timestamp);
    contract.seller_bit = randomBit;
}

#[derive(Serialize, SchemaType)]
struct seller_UpdateListings {
    itemId: u64,
    description: String,
    value: u64,
}
#[receive(contract = "vendor", name = "seller_UpdateListings", parameter = "seller_UpdateListings")]
fn seller_UpdateListings<'a, S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<ContractState<'a, S>, StateApiType = S>,
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

    fn createInitialState<'a, S: HasStateApi>() -> ContractState<'a, S> {
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

    fn s() -> {

    }

    //QUICKCHECK

    //use quickcheck::{Gen, Arbitrary, Testable};
    use quickcheck_macros::quickcheck;
    //use rand::{Rng, SeedableRng, thread_rng};
    //use crate::schema::SizeLength::U64;

    use crate::State::{Accepted, Completed, Counter, Delivered, Dispute, Failed, Null, Rejected, Requested};


    //#[derive(Sized)]
    pub struct ValidPath<'a>(Vec<Choice<'a>>);

    //impl Sized for Vec<Choice> {}

    //enum ParameterTest {
    //    Ctx(HasReceiveContext),
    //    Host,
    //    Amount,
    //}


    pub struct Choice<'a> { state: State, func: &, parameters: Vec<> }

    //clone values
    impl Clone for ValidPath<'_> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    //make arbitrary values
    impl Arbitrary for ValidPath<'_> {
        fn arbitrary(g: &mut Gen) -> Self {

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
            let id = buyer_RequestPurchase(ctx_buyerRequestPurchase_param, host, Amount {micro_ccd: 21});


            // buyer_RequestPurchase(&ctx0, &mut host, Amount { micro_ccd: 10 });

            let contract_name =  "Commerce Contract";
            let id_parameter = to_bytes(&idParameter{
                id: id.clone(),
            });
            let seller_id_randomBit_param = to_bytes(&seller_CounterDispute {
                id: id,
                randomBit: false, // type bool
            });
            let buyer_id_commitment_parameter = to_bytes(&Buyer_id_commitment {
                id: id.clone(),
                commitment: id.clone(),
            });
            let buyer_id_buyerBit_nonce_param =  to_bytes(&buyer_OpenCommitmentParameter {
                id: id.clone(),
                buyerBit: true,
                nonce: id.clone(),
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

            let stateMappings: BTreeMap<State, Vec<Choice>> = BTreeMap::from([
                //(Null, vec![
                //    Choice { state: Requested, func: |host| buyer_RequestPurchase(ctx_buyerRequestPurchase_param, host) },
                //]),
                (Requested, vec![
                    Choice { state: Accepted, func: &seller_ItemWasDelivered, parameters: vec![] },
                    //Choice { state: Accepted, func: &|host| seller_ItemWasDelivered(ctx_idParameter, host) },
                    Choice { state: Failed, func: &buyer_Abort, parameters: vec![ctx_idParameter, host]},
                    Choice { state: Rejected, func: &seller_RejectContract, parameters: vec![ctx_idParameter, host]}
                ]),
                (Rejected, vec![]), //Empty means we should be done
                (Accepted, vec![
                    Choice { state: Failed, func: &|host| buyer_CallTimeout( ctx_idParameter, host)},
                    Choice { state: Delivered, func: &|host| seller_ItemWasDelivered( ctx_idParameter, host)}
                ]),
                (Delivered, vec![
                    Choice { state: Completed, func: &|host| buyer_ConfirmDelivery( ctx_idParameter, host)},
                    Choice { state: Completed, func: &|host| seller_CallTimeout( ctx_idParameter, host)},
                    Choice { state: Dispute, func: &|host| buyer_DisputeDelivery(buyer_id_commitment_parameter, host, Amount {micro_ccd: 21})}
                ]),
                (Dispute, vec![
                    Choice { state: Failed, func: &|host| buyer_CallTimeout( ctx_idParameter, host)},
                    Choice { state: Failed, func: &|host| seller_ForfeitDispute( ctx_idParameter, host)},
                    Choice { state: Counter, func: &|host| seller_CounterDispute( ctx_id_randomBit_param, host, Amount{ micro_ccd: 21})}
                ]),
                (Counter, vec![
                    Choice { state: Failed, func: &buyer_OpenCommitment, parameters: vec![ctx_idParameter, host]},
                    Choice { state: Completed, func: &seller_CallTimeout, parameters: vec![ctx_idParameter, host]}
                ]),
                (Completed, vec![]),
                (Failed, vec![]),
            ]);

            // Start at state Requested
            let mut choices: &Vec<Choice> = stateMappings.get(&State::Null).unwrap();

            // then we add that our path started in state Requested
            let mut path: Vec<Choice> = vec![choices.get(0)];

            // Now we take random decisions from the ones possible
            while choices.len() > 0 {
                let randomIndex = u64::arbitrary(g) % choices.len();
                let Choice {state, func} = choices.get(randomIndex).unwrap();
                path.push(Choice {state, func});
                choices = stateMappings.get(&state).unwrap();
            }

            return ValidPath(path);
        }
    }

    #[quickcheck]
    fn property_check(validPath: ValidPath) -> bool {

        // Need an initial state here?
        let initialContractState: ContractState<dyn HasStateApi> = createInitialState();

        let mut currentContractState = initialContractState;

        let host: &mut TestHost<ContractState<dyn HasStateApi>> = &mut TestHost::new(currentContractState.cloned().state, TestStateBuilder::new());

        for choice in validPath.0.into_iter() {
            assert!(currentContractState.state == choice.state); // THIS IS THE PROPERTY WE ARE TESTING

            choice.clone().get(choice.func)(...choice.parameters); // Call the next function

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