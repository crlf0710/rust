// check-pass
// compile-flags: -F deprecated

#[allow(deprecated)] //~ WARNING allow(deprecated) overruled by outer forbid(deprecated)
fn main() {
}
