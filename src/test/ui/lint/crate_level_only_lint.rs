#![deny(uncommon_codepoints, unused_attributes)]

mod foo {
#![allow(uncommon_codepoints)] //~ ERROR unused lint attribute: `uncommon_codepoints`

#[allow(uncommon_codepoints)]
const BAR: f64 = 0.000001; //~^ ERROR unused lint attribute: `uncommon_codepoints`

}

#[allow(uncommon_codepoints)]
fn main() { //~^ ERROR unused lint attribute: `uncommon_codepoints`
}
