#![allow(dead_code)]
#![allow(deprecated)]
#![allow(clippy::enum_variant_names)]

include!(concat!(env!("OUT_DIR"), "/daphne.rs"));

pub mod sc {
    #![allow(dead_code)]
    #![allow(clippy::enum_variant_names)]

    include!(concat!(env!("OUT_DIR"), "/daphne.sc.rs"));
}
