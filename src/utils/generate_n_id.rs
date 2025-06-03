use crate::utils::now;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use std::cell::RefCell;

thread_local! {
    static SALT_RNG: RefCell<ChaCha8Rng> = {
        let seed = now();
        RefCell::new(ChaCha8Rng::seed_from_u64(seed))
    };
}

// #[inline]
// pub fn generate_u64_id() -> u64 {
//     let millis = now() as u64;
//     let rand16 = SALT_RNG.with(|rng| rng.borrow_mut().next_u32() as u16);
//     (millis << 16) | (rand16 as u64)
// }

#[inline]
pub fn generate_u32_id() -> u32 {
    let millis = now() as u32;
    let rand10 = SALT_RNG.with(|rng| rng.borrow_mut().next_u32() & 0x03FF);
    (millis << 10) | (rand10 as u32)
}
