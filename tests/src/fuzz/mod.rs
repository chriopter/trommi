//! The fuzz harnesses (README "Fuzzing"): one `run` per target, shared by the libFuzzer targets in
//! `tests/fuzz/` and by the property tests and corpus replay of `tests/fuzz_props.rs`. A `run` panics on a
//! broken property; on any input it never hangs and never touches the network.

pub mod board;
pub mod decode;

use trommi_core::crypto::Entropy;
use trommi_core::Error;

/// The fuzzer's bytes as a random source: what the harness draws, the fuzzer chose. Once they run out it
/// gives `Error::Entropy`, and a harness stops making things.
pub struct Bytes<'a> {
    data: &'a [u8],
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// The bytes not drawn yet.
    pub fn rest(&self) -> &'a [u8] {
        self.data
    }
}

impl Entropy for Bytes<'_> {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), Error> {
        if self.data.len() < out.len() {
            self.data = &[];
            return Err(Error::Entropy);
        }
        let (now, later) = self.data.split_at(out.len());
        out.copy_from_slice(now);
        self.data = later;
        Ok(())
    }
}
pub mod connector;
pub mod devices;
pub mod flow;
pub mod hub;
