#![deny(uncommon_codepoints)]

mod foo {
#![allow(uncommon_codepoints)] //~ ERROR allow(uncommon_codepoints)

#[allow(uncommon_codepoints)]
const BAR: f64 = 0.000001; //~^ ERROR allow(uncommon_codepoints)

}

#[allow(uncommon_codepoints)]
fn main() { //~^ ERROR allow(uncommon_codepoints)
}
