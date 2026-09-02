pub mod loader;
pub mod types;

use std::sync::OnceLock;

pub use loader::*;
pub use types::*;

// ============================================================================
// Global Singletons (`OnceLock`)
// ============================================================================

static JWT_CONFIG: OnceLock<JwtConfig> = OnceLock::new();
static EMAIL_CONFIG: OnceLock<EmailConfig> = OnceLock::new();
static FRONTEND_CONFIG: OnceLock<FrontendConfig> = OnceLock::new();
static GITHUB_CONFIG: OnceLock<GitHubConfig> = OnceLock::new();
static MARKET_CONFIG: OnceLock<MarketConfig> = OnceLock::new();
static OPENROUTER_CONFIG: OnceLock<OpenRouterConfig> = OnceLock::new();
static HTTP_CONFIG: OnceLock<HttpConfig> = OnceLock::new();

impl JwtConfig {
    pub fn init(cfg: JwtConfig) {
        JWT_CONFIG.set(cfg).expect("JwtConfig already initialized");
    }

    pub fn get() -> &'static JwtConfig {
        JWT_CONFIG.get().expect("JwtConfig not initialized")
    }
}

impl EmailConfig {
    pub fn init(cfg: EmailConfig) {
        EMAIL_CONFIG
            .set(cfg)
            .expect("EmailConfig already initialized");
    }

    pub fn get() -> &'static EmailConfig {
        EMAIL_CONFIG.get().expect("EmailConfig not initialized")
    }
}

impl HttpConfig {
    pub fn init(cfg: HttpConfig) {
        HTTP_CONFIG
            .set(cfg)
            .expect("HttpConfig already initialized");
    }

    pub fn get() -> &'static HttpConfig {
        HTTP_CONFIG.get().expect("HttpConfig not initialized")
    }
}

impl FrontendConfig {
    pub fn init(cfg: FrontendConfig) {
        FRONTEND_CONFIG
            .set(cfg)
            .expect("FrontendConfig already initialized");
    }

    pub fn get() -> &'static FrontendConfig {
        FRONTEND_CONFIG
            .get()
            .expect("FrontendConfig not initialized")
    }
}

impl OpenRouterConfig {
    pub fn init(cfg: OpenRouterConfig) {
        OPENROUTER_CONFIG
            .set(cfg)
            .expect("OpenRouterConfig already initialized");
    }

    pub fn get() -> &'static OpenRouterConfig {
        OPENROUTER_CONFIG
            .get()
            .expect("OpenRouterConfig not initialized")
    }
}

impl GitHubConfig {
    pub fn init(cfg: GitHubConfig) {
        GITHUB_CONFIG
            .set(cfg)
            .expect("GitHubConfig already initialized");
    }

    pub fn get() -> &'static GitHubConfig {
        GITHUB_CONFIG.get().expect("GitHubConfig not initialized")
    }
}

impl MarketConfig {
    pub fn init(cfg: MarketConfig) {
        MARKET_CONFIG
            .set(cfg)
            .expect("MarketConfig already initialized");
    }

    pub fn get() -> &'static MarketConfig {
        MARKET_CONFIG.get().expect("MarketConfig not initialized")
    }
}
