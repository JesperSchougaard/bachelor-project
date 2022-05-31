///! Rust file containing property-based testing

use crate::*;

#[concordium_cfg_test]
mod tests {
    use std::borrow::{Borrow, BorrowMut};
    use std::collections::BTreeMap;
    use concordium_std::test_infrastructure::{TestStateBuilder, TestReceiveContext};
    use super::*;
    use crate::tests::test_utils::*;
    use group::GroupEncoding;
    use k256::{ProjectivePoint, Scalar};
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;
    use sha2::{Digest, Sha256};
    use crate::types::VotingPhase;

    // Make a vector of accounts into a map of accounts and their behavior
    fn make_behavior_map(accounts: Vec<AccountAddress>)
    -> BTreeMap<AccountAddress, Behavior> {
        let mut behavior_map: BTreeMap<AccountAddress, Behavior> = BTreeMap::new();
        for acc in accounts {
            behavior_map.insert(acc, Behavior {
                registered: false,
                committed: false,
                voted: false
            });
        }
        return behavior_map;
    }

    /*
    // Make a list of keys
    fn make_keys(number_of_accounts: usize)
        -> (Vec<(Scalar, ProjectivePoint)>, Vec<ProjectivePoint>) {
        let mut voting_key_pairs = vec![];
        let mut reconstructed_keys = vec![];
        let mut keys = vec![];
        for _ in 0..number_of_accounts {
            let (x, g_x) = off_chain::create_votingkey_pair();
            voting_key_pairs.push((x, g_x));
            keys.push(g_x);
        }
        for i in 0..number_of_accounts {
            let (_, g_x) = voting_key_pairs.get(i).unwrap();
            reconstructed_keys.push(off_chain::compute_reconstructed_key(&keys, *g_x));
        }
        return (voting_key_pairs, reconstructed_keys)
    }
    */


    #[derive(Clone, Debug, Copy)]
    pub struct Behavior {
        registered: bool,
        committed: bool,
        voted: bool,
    }

    #[derive(Clone, Debug, Copy)]
    pub enum Funcs {
        Register,
        Commit,
        Vote,
        Result,
        ChangePhase
    }

    impl Funcs {
        fn make_vec() -> Vec<Funcs> {
            vec![Funcs::Register, Funcs::Commit, Funcs::Vote, Funcs::Result, Funcs::ChangePhase]
        }
    }

    impl Arbitrary for Funcs {
        fn arbitrary(g: &mut Gen) -> Self {
            let func_vec = Funcs::make_vec();
            let random_index = (u64::arbitrary(g) as usize) % func_vec.len();
            let func = *func_vec.get(random_index).unwrap();
            func
        }
    }

    pub struct Commands(Vec<Funcs>);

    impl Clone for Commands {
        fn clone(&self) -> Self {
            Commands(self.0.clone())
        }
    }

    impl Arbitrary for Commands {
        fn arbitrary(g: &mut Gen) -> Self {
            let commands = vec![];
            for _ in 1..4 {
                let func_vec = Funcs::make_vec();
                let random_index = (u64::arbitrary(g) as usize) % func_vec.len();
                let func: Funcs = *func_vec.get(random_index).unwrap();
                commands.clone().push(func);
            }
            return Commands(commands);
        }
    }

    #[quickcheck]
    fn test_prop_8(ran_deposit: u8, ran_timeout: u64, functions: Vec<Vec<Funcs>>) -> bool {
        //println!("Random deposit{:?}", ran_deposit);
        // println!("functions: {:?}", functions
        let deposit = (ran_deposit as u64) + 1;
        let timeout = (ran_timeout % 100) + 2; // timeout between 2 and 27
        let number_of_accounts = functions.len();
        // discard tests outside of 3 < x < 8 accounts
        if number_of_accounts < 3 { return true };
        if number_of_accounts > 8 { return true };
        //println!("Started test");
        //println!("Random timeout {:?}", timeout);
        //println!("number of accounts {:?}", number_of_accounts);


        // Setup vote config and accounts
        let (accounts, vote_config, merkle_tree) =
            setup_test_config(number_of_accounts as i32, Amount {micro_ccd: deposit});

        // helper function for keys should be done, uncomment stuff to make it simpler in the test code
        //let (mut voting_key_pairs, mut reconstructed_keys) = make_keys(number_of_accounts);
        let mut voting_key_pairs = vec![];
        let mut reconstructed_keys = vec![];
        let mut keys = vec![];
        for _ in 0..number_of_accounts {
            let (x, g_x) = off_chain::create_votingkey_pair();
            voting_key_pairs.push((x, g_x));
            keys.push(g_x);
        }
        for i in 0..number_of_accounts {
            let (_, g_x) = voting_key_pairs.get(i).unwrap();
            reconstructed_keys.push(off_chain::compute_reconstructed_key(&keys, *g_x));
        }

        // Create map to keep track of behavior of random accounts
        let mut behavior_map = make_behavior_map(accounts.clone());

        // Create state_builder and use vote_config to initialize contract
        let init_parameter = to_bytes(&vote_config);
        let init_ctx = setup_init_context(&init_parameter);
        let mut statebuilder = TestStateBuilder::new();
        let voting_state = setup(
            &init_ctx,
            &mut statebuilder
        ).unwrap();
        let (_, mut host) =
            setup_receive_context(None,
                                  *accounts.first().unwrap(),
                                  voting_state,
                                  statebuilder);

        // Accounts potentially tries to register, commit, vote, tally result and change phases
        let mut func_call_index = 0;
        while func_call_index < timeout {
            for (account_index, func_list) in functions.clone().into_iter().enumerate() {
                let cur_addr_acc = Address::Account(*accounts.get(account_index).unwrap());
                let cur_acc = accounts.get(account_index).unwrap();
                let mut ctx = TestReceiveContext::empty();
                ctx.metadata_mut().set_slot_time(Timestamp::from_timestamp_millis(1));
                ctx.set_sender(cur_addr_acc);
                host.set_self_balance(Amount::from_micro_ccd(u64::MAX));
                let func_to_call = func_list.get(func_call_index as usize);
                if func_to_call.is_none() { continue }
                match func_to_call.unwrap() {
                    Funcs::Register => {
                        let (x, g_x) = voting_key_pairs.get(account_index).unwrap();
                        let register_message = to_bytes(&RegisterMessage {
                            voting_key: g_x.to_bytes().to_vec(),
                            voting_key_zkp: off_chain::create_schnorr_zkp(*g_x, *x),
                            merkle_proof: off_chain::create_merkle_proof(*cur_acc, &merkle_tree),
                        });

                        ctx.set_parameter(&register_message);
                        let result = register(&ctx, &mut host, Amount { micro_ccd: deposit });
                        if result.is_ok() {
                            behavior_map.get_mut(cur_acc).unwrap().registered = true;
                        }
                    },
                    Funcs::Commit => {
                        let g_y = reconstructed_keys.get(account_index).unwrap();
                        let (x, _) = voting_key_pairs.get(account_index).unwrap();
                        let commitment = off_chain::commit_to_vote(&x, &g_y, ProjectivePoint::GENERATOR);

                        //println!("asdflkasdf {:?}", ProjectivePoint::GENERATOR);

                        let commitment_message = to_bytes(&CommitMessage {
                            reconstructed_key: g_y.to_bytes().to_vec(),
                            commitment,
                        });

                        ctx.set_parameter(&commitment_message);

                        let result = commit(&ctx, &mut host);
                        if result.is_ok() {
                            behavior_map.get_mut(cur_acc).unwrap().committed = true;
                        }
                    },

                    Funcs::Vote => {
                        let (x, g_x) = voting_key_pairs.get(account_index).unwrap();
                        let g_y = reconstructed_keys.get(account_index).unwrap();


                        //let (x, g_x) = off_chain::create_votingkey_pair();
                        //let g_y = off_chain::compute_reconstructed_key(&vec![g_x.clone()], g_x.clone());

                        let one_in_two_zkp_account1 =
                            off_chain::create_one_in_two_zkp_no(*g_x, g_y.clone(), x.clone());

                        let vote_message = &VoteMessage {
                            vote: ((g_y.clone() * x.clone()) + ProjectivePoint::GENERATOR)
                                .to_bytes()
                                .to_vec(),
                            vote_zkp: one_in_two_zkp_account1,
                        };

                        let vote_message_bytes = to_bytes(vote_message);
                        ctx.set_parameter(&vote_message_bytes);

                        let result = vote(&ctx, &mut host);
                        if result.is_ok() {
                            behavior_map.get_mut(cur_acc).unwrap().voted = true;
                        } else if host.state().clone().voting_phase == types::VotingPhase::Vote
                            && behavior_map.get(cur_acc).unwrap().committed {
                            //println!("Error in vote {:?}", result.err())
                        }
                    },
                    Funcs::Result => {
                        let _ = result(&ctx, &mut host);
                    },
                    Funcs::ChangePhase => {
                        //change_phase_precondtions()

                        let _ = change_phase(&ctx, &mut host);
                        //change_phase_postcondtions()
                    },
                }
            }
            //println!("voting phase: {:?}", host.state().voting_phase);
            func_call_index += 1;
        }

        // Timeout is reached, call change phase. We may or may not be in result
        let bur_host = host.borrow().clone();
        let cur_phase = bur_host.state().voting_phase;

        let change_phase_caller = accounts.first().unwrap();
        let mut phase_caller_is_honest = false;
        if cur_phase != VotingPhase::Result || cur_phase != VotingPhase::Abort {
            let mut state_builder = TestStateBuilder::new();
            let (mut ctx, _) =
                setup_receive_context(None,
                                      *accounts.first().unwrap(),
                                      setup(
                                          &init_ctx,
                                          &mut state_builder
                                      ).unwrap(),
                                      state_builder);
            // Set contracts balance to max so we can transfer currency back
            host.set_self_balance(Amount::from_micro_ccd(u64::MAX));
            ctx.set_sender(Address::Account(*change_phase_caller));
            ctx.set_metadata_slot_time(Timestamp::from_timestamp_millis(301));
            let result = change_phase(&ctx, &mut host);
            let result2 = change_phase(&ctx, &mut host);
            //println!("1 Phase: {:?}", cur_phase);
            //println!("2 Phase: {:?}", cur_phase);
            //println!("3 Phase: {:?}", cur_phase);
            //println!("Result: {:?}", result);
            phase_caller_is_honest = behavior_map.clone().get(change_phase_caller).unwrap().clone().voted;
        }

        //println!("Phase: {:?}", cur_phase);
        let bur_host = host.borrow().clone();
        let cur_phase = bur_host.state().voting_phase;
        println!("cur_phase: {:?}", cur_phase);
        assert!(cur_phase == VotingPhase::Result ||
                cur_phase == VotingPhase::Abort,
                cur_phase);





        // Assertions about the rest of the accounts
        let transfers = host.get_transfers();
        let mut transfer_map = transfers.clone().into_iter().fold(BTreeMap::new(), |reduced_result, (acc, amount)| {
            let acc_transfer_sum_so_far = reduced_result.get(&acc).unwrap_or_else(|| &0);
            let mut temp = reduced_result.clone();
            temp.insert(acc, acc_transfer_sum_so_far + amount.micro_ccd);
            return temp;
        });
        let honest_accounts = accounts.clone().into_iter().filter(
            |acc| behavior_map.clone().get_mut(acc).unwrap().voted
        );

        for acc in behavior_map.clone() {
            println!("Account behavior: {:?}", acc.1);
        }
        println!("Transfer map with len: {:?}", transfer_map.len());
        println!("Transfer list with len: {:?}", transfers.len());
        for tran in transfer_map.values() {
            println!("Transfer: {:?}", tran)
        }

        println!("Honest acc: {:?}", honest_accounts.clone().count());


        // Assertions about the net gain of "some_acc"
        if phase_caller_is_honest {
            // The contract will refund (=deposit) and reward change_phase_caller for calling change_phase (=deposit)
            // So we expect that the contract has transfered a total of 2*deposit to change_phase_caller
            assert_eq!(*transfer_map.get(change_phase_caller).unwrap(), 2*deposit);
            transfer_map.insert(*change_phase_caller, deposit); // Set the value for the next assert
        }
        honest_accounts.into_iter().for_each(|acc|
            assert_eq!(*transfer_map.get(&acc).unwrap(), deposit, "321")
        );
        true
    }


}




