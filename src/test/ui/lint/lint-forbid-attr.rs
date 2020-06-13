// check-pass
#![forbid(deprecated)]

#[allow(deprecated)]
//~^ WARNING allow(deprecated) overruled by outer forbid(deprecated)
fn main() {
}
