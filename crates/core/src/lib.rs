pub mod capture;
pub mod error;
pub mod export;
pub mod model;
pub mod proxy;
pub mod replay;
pub mod rules;
pub mod security;
pub mod store;
#[cfg(test)]
mod tests;

pub use capture::*;
pub use error::*;
pub use export::*;
pub use model::*;
pub use proxy::*;
pub use replay::*;
pub use rules::*;
pub use security::*;
pub use store::*;

pub fn init_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
