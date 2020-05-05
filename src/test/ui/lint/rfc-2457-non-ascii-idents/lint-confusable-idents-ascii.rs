#![deny(confusable_idents_ascii)]
#![allow(confusable_idents)]

fn main() {
    let s1 = "rust";
    let sl = "rust2";  //~ ERROR identifier pair considered confusable
}
