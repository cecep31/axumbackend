pub mod loader;
pub mod types;

use std::sync::OnceLock;

pub use loader::*;
pub use types::*;

// ============================================================================
// Global Singletons (`OnceLock`)
// ============================================================================

/// Gives a config section a process-wide `init` (once, at startup) and `get`.
macro_rules! global_config {
    ($($ty:ident => $cell:ident),* $(,)?) => {
        $(
            static $cell: OnceLock<$ty> = OnceLock::new();

            impl $ty {
                pub fn init(cfg: $ty) {
                    $cell
                        .set(cfg)
                        .expect(concat!(stringify!($ty), " already initialized"));
                }

                pub fn get() -> &'static $ty {
                    $cell
                        .get()
                        .expect(concat!(stringify!($ty), " not initialized"))
                }
            }
        )*
    };
}

impl Config {
    /// Publishes the sections that are read through `XxxConfig::get()`.
    /// Must be called exactly once, at startup.
    pub fn init_globals(&self) {
        JwtConfig::init(self.jwt.clone());
        EmailConfig::init(self.email.clone());
        FrontendConfig::init(self.frontend.clone());
        GitHubConfig::init(self.github.clone());
        MarketConfig::init(self.market.clone());
        OpenRouterConfig::init(self.openrouter.clone());
        HttpConfig::init(self.http.clone());
    }
}

global_config! {
    JwtConfig => JWT_CONFIG,
    EmailConfig => EMAIL_CONFIG,
    FrontendConfig => FRONTEND_CONFIG,
    GitHubConfig => GITHUB_CONFIG,
    MarketConfig => MARKET_CONFIG,
    OpenRouterConfig => OPENROUTER_CONFIG,
    HttpConfig => HTTP_CONFIG,
}
