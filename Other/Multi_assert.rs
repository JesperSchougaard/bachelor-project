use std::fmt::Display;

struct Assert<T: Eq + Display + Copy> {
    left: T,
    right: T,
    result: bool,
    line: u32
}

struct MultiAssert<T: Eq + Display + Copy> {
    asserts: Vec<Assert<T>>
}

impl<T: Eq + Display + Copy> MultiAssert<T> {
    fn new() -> MultiAssert<T> {
        MultiAssert {
            asserts: Vec::new()
        }
    }

    fn add_assert(&mut self, left: T, right: T, line: u32) -> bool {
        let result = left == right;
        let assert = Assert {
            left: left,
            right: right,
            result: result,
            line: line
        };
        self.asserts.push(assert);
        result
    }

    fn run_asserts(&self) {
        let mut fails = 0;
        for assert in &self.asserts {
            if !assert.result {
                println!("Line {}: {} != {}", assert.line, assert.left, assert.right);
                fails += 1;
            }
        }
        if fails > 0 {
            panic!("Multi_assertion failed with {} assertion fail{}", fails, if fails == 1 { "" } else { "s" });
        }
    }

    fn show_asserts(&self) {
        println!("Showing {} assert{}:", self.asserts.len(), if self.asserts.len() == 1 { "" } else { "s" });
        for assert in &self.asserts {
            println!("Line {}: Assert {} == {}", assert.line, assert.left, assert.right);
        }
    }
}


// macro_rules! add_assert_to {
//     ($s:item; $arg1:expr, $arg2:expr) => { &s.add_assert($arg1, $arg2, line!());}
// }
// add_assert_to!(multi_assert.add_assert; 1, 2);


fn main(){
    let mut multi_assert = MultiAssert::new();
    multi_assert.add_assert(1, 2, line!());
    multi_assert.add_assert(1, 1, line!());
    multi_assert.add_assert(2, 2, line!());
    multi_assert.add_assert(2, 1, line!());

    multi_assert.show_asserts();

    println!("\n\n");

    multi_assert.run_asserts();
}