// Forbidding a group (here, `unused`) overrules subsequent allowance of both
// the group, and an individual lint in the group (here, `unused_variables`);
// and, forbidding an individual lint (here, `non_snake_case`) overrules
// subsequent allowance of a lint group containing it (here, `nonstandard_style`). See
// Issue #42873.

#![forbid(unused, non_snake_case)]

#[allow(unused_variables)] //~ WARNING overruled
fn foo() {} //~ ERROR function is never used: `foo`

#[allow(unused)] //~ WARNING overruled
fn bar() {} //~ ERROR function is never used: `bar`

#[allow(nonstandard_style)] //~ WARNING overruled
fn main() {
    println!("hello forbidden world")
}
