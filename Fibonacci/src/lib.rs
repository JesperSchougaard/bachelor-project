#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(warnings, unused)]


use concordium_std::{collections::BTreeMap, *};
use core::fmt::Debug;
use std::sync::atomic::{AtomicU8, Ordering};
use concordium_std::test_infrastructure::{ActionsTree, ReceiveContextTest};

static ADDRESS_COUNTER: AtomicU8 = AtomicU8::new(0);

fn new_account() -> AccountAddress {
    let account = AccountAddress([ADDRESS_COUNTER.load(Ordering::SeqCst); 32]);
    ADDRESS_COUNTER.fetch_add(1, Ordering::SeqCst);
    account
}

fn new_account_ctx<'a>() -> (AccountAddress, ReceiveContextTest<'a>) {
    let account = new_account();
    let ctx = new_ctx(account, account);
    (account, ctx)
}

fn new_ctx<'a>(
    owner: AccountAddress,
    sender: AccountAddress,
) -> ReceiveContextTest<'a> {
    let mut ctx = ReceiveContextTest::empty();
    ctx.set_sender(Address::Account(sender));
    ctx.set_owner(owner);
    ctx.set_self_address(ContractAddress { index: 0, subindex: 0 });
    ctx
}


#[contract_state(contract = "fib")]
#[derive(Serialize, SchemaType)]
pub struct State {
    result: u64,
}

#[contract_state(contract = "fib")]
#[derive(Serialize)]
pub struct FibNum {
    number: u64,
}

#[init(contract = "fib")]
#[inline(always)]
fn contract_init(_ctx: &impl HasInitContext<()>) -> InitResult<State> {
    let state = State {
        result: 0,
    };
    Ok(state)
}

// Add the the nth Fibonacci number F(n) to this contract's state.
// This is achieved by recursively sending messages to this receive method,
// corresponding to the recursive evaluation F(n) = F(n-1) + F(n-2) (for n>1).
#[inline(always)]
#[receive(contract = "fib", name = "receive", parameter = "FibNum")]
fn contract_receive<A: HasActions>(
    ctx: &impl HasReceiveContext<()>,
    state: &mut State,
) -> ReceiveResult<A> {

    let mut pointer_state = State {result: 0};
    //let result: ReceiveResult<ActionsTree> = contract_receive_calc_fib(&ctx1, Amount::zero(), state);
    //println!("Test: {}", result.is_ok());

    // Try to get the parameter (64bit unsigned integer).
    let fibnum: FibNum = ctx.parameter_cursor().get()?;
    let n = fibnum.number;

    //if n == 20 {    // Intentionally implemented fib incorrectly
    //    return Ok(A::accept())
    //};

    //println!("{}", state.result);
    //println!("n: {}", n);
    if n <= 1 {
        //println!("B: {}", n);
        state.result += 1;
        Ok(A::accept())
    } else {
        let self_address = ctx.self_address();
        //println!("C: {}", n);
        let number1 = FibNum { number: n-1 };
        let number2 = FibNum { number: n-2 };
        let bytes1 = to_bytes(&number1);
        let bytes2 = to_bytes(&number2);

        let mut ctx1: ReceiveContextTest = new_ctx(ctx.owner(), ctx.owner());
        ctx1.set_parameter(&bytes1);
        let mut ctx2: ReceiveContextTest = new_ctx(ctx.owner(), ctx.owner());
        ctx2.set_parameter(&bytes2);

        let result1: ReceiveResult<ActionsTree> = contract_receive(&ctx1, state);
        let result2: ReceiveResult<ActionsTree> = contract_receive(&ctx2, state);

        //println!("1: {}", result1.is_ok());
        //println!("2: {}", result2.is_ok());
        result1.is_ok();
        result2.is_ok();

        Ok(A::accept())
    }
}

// Calculates the nth Fibonacci number where n is the given amount and sets the
// state to that number.
#[inline(always)]
#[receive(contract = "fib", name = "receive_calc_fib", payable)]
fn contract_receive_calc_fib<A: HasActions>(
    _ctx: &impl HasReceiveContext<()>,
    amount: Amount,
    state: &mut State,
) -> ReceiveResult<A> {
    println!("[contract_receive_calc_fib]");
    state.result = fib(amount.micro_ccd);
    Ok(A::accept())
}

// Recursively and naively calculate the nth Fibonacci number.
fn fib(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        fib(n - 1) + fib(n - 2)
    }
}


#[cfg(test)]
extern crate quickcheck;
#[cfg(test)]
#[macro_use(quickcheck)]
extern crate quickcheck_macros;
extern crate concordium_std;
#[cfg(test)]
mod tests {
    use quickcheck::{Gen, Arbitrary, Testable, TestResult};
    use super::*;
    use test_infrastructure::*;
    use concordium_std::*;
    use rand::{thread_rng, Rng};

    #[derive(Debug, Clone)]
    struct u64InRange(u64);

    // limit numbers to 22 since fibonacci implementation is slow (exponential)
    impl Arbitrary for u64InRange {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut rng = thread_rng();
            u64InRange(rng.gen_range(0..22))  //    http://mathman.biz/html/30fibnumbers.html
        }
    }

    #[quickcheck]
    fn post_condition(something: u64InRange) -> bool {
        let mut n: u64 = something.0;

        let (bob, mut bob_ctx) = new_account_ctx();

        let fibnum_test = FibNum { number: n };
        let test = to_bytes(&fibnum_test);
        bob_ctx.set_parameter(&test);

        let mut pointer_state = State {result: 0};

        let result: ReceiveResult<ActionsTree> = contract_receive(&bob_ctx, &mut pointer_state);
        //println!("n: {}, Fib: {}, is equal: {}", n, pointer_state.result, fib(fibnum_test.number) == pointer_state.result );
        //println!("E: {}", result.is_ok());

        assert_eq!(pointer_state.result, fib(n));
        true
    }


    #[quickcheck]
    fn metamorphic_properties(something: u64InRange) -> bool {
        let (acc, mut ctx) = new_account_ctx();
        let mut n: u64 = something.0;

        let fibnum_a = FibNum { number: n };
        let fibnum_a1 = FibNum { number: n+1 };
        let fibnum_a2 = FibNum { number: n+2 };

        // f(a)
        let a = to_bytes(&fibnum_a);
        ctx.set_parameter(&a);
        let mut pointer_state_a = State {result: 0};
        let _: ReceiveResult<ActionsTree> = contract_receive(&ctx, &mut pointer_state_a);
        let result_a: u64 = pointer_state_a.result;

        // f(a+1)
        let a1 = to_bytes(&fibnum_a1);
        ctx.set_parameter(&a1);
        let mut pointer_state_a1 = State {result: 0};
        let _: ReceiveResult<ActionsTree> = contract_receive(&ctx, &mut pointer_state_a1);
        let result_a1 = pointer_state_a1.result;

        // f(a+2)
        let a2 = to_bytes(&fibnum_a2);
        ctx.set_parameter(&a2);
        let mut pointer_state_a2 = State {result: 0};
        let _: ReceiveResult<ActionsTree> = contract_receive(&ctx, &mut pointer_state_a2);
        let result_a2 = pointer_state_a2.result;

        assert!(result_a >= 1, "property 3");                      // property 3, f(a) >= 1
        assert_eq!(result_a + result_a1, result_a2, "property 4"); // property 4, f(a) + f(a+1) = f(a+2)
        assert_eq!(result_a2 - result_a1, result_a, "property 5"); // property 5, f(a+2) - f(a+1) = f(a)
        assert!(result_a <= result_a1, "property 6");              // property 6, f(a) <= f(a+1)
        true
    }

}
