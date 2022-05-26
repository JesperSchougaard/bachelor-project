use concordium_std::*;

#[derive(Serialize, PartialEq, Eq, Debug)]
enum PiggyBankState {
    Intact,
    Smashed,
}
#[derive(Debug, PartialEq, Eq, Reject)]
enum SmashError {
    NotOwner,
    AlreadySmashed,
}

#[init(contract = "PiggyBankTesting")]
fn piggy_init(_ctx: &impl HasInitContext) -> InitResult<PiggyBankState> {
    Ok(PiggyBankState::Intact)
}


#[receive(contract = "PiggyBankTesting", name = "insert", payable)]
fn piggy_insert<A: HasActions>(
    _ctx: &impl HasReceiveContext,
    _amount: Amount,
    state: &mut PiggyBankState,
) -> ReceiveResult<A> {
    ensure!(*state == PiggyBankState::Intact);
    Ok(A::accept())
}

#[receive(contract = "PiggyBankTesting", name = "smash")]
fn piggy_smash<A: HasActions>(
    ctx: &impl HasReceiveContext,
    state: &mut PiggyBankState,
) -> Result<A, SmashError> {
    let owner = ctx.owner();
    let sender = ctx.sender();
    ensure!(sender.matches_account(&owner), SmashError::NotOwner);
    ensure!(*state == PiggyBankState::Intact, SmashError::AlreadySmashed);

    *state = PiggyBankState::Smashed; //smash!
    let balance = ctx.self_balance();
    Ok(A::simple_transfer(&owner, balance))
}
#[concordium_cfg_test]
mod tests {
    use super::*;
    use test_infrastructure::*;
    #[concordium_test]
    fn test_init() {
        let ctx = InitContextTest::empty();
        let state_result = piggy_init(&ctx);
        let state = state_result.expect_report("Contract Initialization results in error.");
        claim_eq!(
            state,
            PiggyBankState::Intact,
            "Piggy bank state should be intact after initialization."
        );
    }
    #[concordium_test]
    fn test_insert_intact() {
        let ctx = ReceiveContextTest::empty();
        let amount = Amount::from_micro_ccd(100);
        let mut state = PiggyBankState::Intact;

        let actions_result: ReceiveResult<ActionsTree> = piggy_insert(&ctx, amount, &mut state);

        let actions = actions_result.expect_report("Inserting CCD results in error.");

        claim_eq!(
            actions,
            ActionsTree::accept(),
            "No action should be produced."
        );
        claim_eq!(
            state,
            PiggyBankState::Intact,
            "Piggy bank state should still be intact."
        );
    }
    #[concordium_test]
    fn test_insert_smashed() {
        let ctx = ReceiveContextTest::empty();
        let amount = Amount::from_micro_ccd(100);
        let mut state = PiggyBankState::Smashed;

        let actions_result: ReceiveResult<ActionsTree> = piggy_insert(&ctx, amount, &mut state);

        claim!(
            actions_result.is_err(),
            "Contract prevents us from inserting into smashed piggy"
        );

        claim_eq!(
            state,
            PiggyBankState::Smashed,
            "Piggy bank state should be smashed!"
        );
    }

    #[concordium_test]
    fn test_smash() {
        let mut ctx = ReceiveContextTest::empty();
        let owner = AccountAddress([0u8; 32]);
        ctx.set_owner(owner);
        let sender = Address::Account(owner);
        ctx.set_sender(sender);
        let balance = Amount::from_micro_ccd(100);
        ctx.set_self_balance(balance);
        let mut state = PiggyBankState::Intact;

        let actions_result: Result<ActionsTree, SmashError> = piggy_smash(&ctx, &mut state);

        let actions = actions_result.expect_report("Inserting CCD results in error.");

        claim_eq!(actions, ActionsTree::simple_transfer(&owner, balance));
        claim_eq!(state, PiggyBankState::Smashed);
    }

    #[concordium_test]
    fn test_smash_intact_not_owner() {
        let mut ctx = ReceiveContextTest::empty();
        let owner = AccountAddress([0u8; 32]);
        ctx.set_owner(owner);
        let sender = Address::Account(AccountAddress([1u8; 32]));
        ctx.set_sender(sender);
        let balance = Amount::from_micro_ccd(100);
        ctx.set_self_balance(balance);
        let mut state = PiggyBankState::Intact;

        let actions_result: Result<ActionsTree, SmashError> = piggy_smash(&ctx, &mut state);

        let err = actions_result.expect_err_report("Contract is expected to fail.");
        claim_eq!(
            err,
            SmashError::NotOwner,
            "Expected to fail with error NotOwner."
        );
    }
}
