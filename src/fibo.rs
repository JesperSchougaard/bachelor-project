use concordium_std::*;
use concordium_std::schema::Type::Timestamp;


const ONE_DAY: u64 = 86400000; // milliseconds in a day

#[derive(Serialize, PartialEq, Eq, Debug)]
struct FibonacciDeposits {
    //deposits: u64,
    second_last_deposit_address: AccountAddress,
    last_deposit_address: AccountAddress,
    last_deposit_time: Timestamp,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
enum ContractStatus {
    Live,
    Dead,
}

#[init(contract = "Fibonacci")]
fn fibonacci_init(_ctx: &impl HasInitContext) -> InitResult<ContractStatus> {
    FibonacciDeposits.deposits = 0;
    Ok(ContractStatus::Live)
}

fn fibonnaci_payout() {
    if Timestamp.duration_between(FibonacciDeposits.last_deposit_time()) > ONE_DAY {
        //send money

    }
}



#[receive(contract = "Fibonacci", name = "insert", payable)]
fn fibonnaci_deposit<A: HasActions>(
    ctx: &impl HasReceiveContext,
    amount: Amount,
    status: &mut ContractStatus,
) -> ReceiveResult<A> {
    ensure!(*status == ContractStatus::Live);
    ensure!(check_if_fibonacci(amount, FibonacciDeposits.deposits+1));

    FibonacciDeposits.deposits += 1;
    FibonacciDeposits.last_deposit = ctx.invoker();
    FibonacciDeposits.last_deposit_time = Timestamp.timestamp_millis();

    Ok(A::accept())
}

fn check_if_fibonacci(amount: u64, deposits: u64) -> Bool {
    /*
        fn fib(n: u8) -> u64 {
            let mut prev: u64 = 0;
            let mut curr: u64 = 1;
            for _ in 1..n {
                let next = prev + curr;
                prev = curr;
                curr = next;
            }
            curr
        }
    */

    fn fib(n: u64) -> u64 {
        match n {
            1 | 2 => 1,
            _ => fib(n-1) + fib(n-2),
        }
    }
    return fib(deposits) == amount;
}

