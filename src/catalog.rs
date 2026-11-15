//! Built-in list of recommended providers, mostly ones with a free tier.
//!
//! Free tiers and model names change often, so the TUI fetches each provider's
//! live model list; `models` here are only the suggested picks.
//! Last checked: 15 November 2026 (OpenRouter and NVIDIA against their live lists).

use crate::config::Wire;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Free,
    Local,
    Paid,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Free => "FREE",
            Tier::Local => "LOCAL",
            Tier::Paid => "PAID",
        }
    }
}

/// Which of a provider's models cost nothing.
#[derive(Clone, Copy)]
pub enum FreeRule {
    All,
    None,
    /// model id contains this (e.g. "flash")
    Contains(&'static str),
    /// OpenRouter style: id ends in ":free" or pricing is zero
    OpenRouter,
    /// exactly these ids
    Only(&'static [&'static str]),
}

pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    pub tier: Tier,
    pub wire: Wire,
    pub base_url: &'static str,
    /// where to create an API key; empty = no key needed
    pub key_url: &'static str,
    pub blurb: &'static str,
    pub limits: &'static str,
    pub models: &'static [&'static str],
    pub models_url: Option<&'static str>,
    pub free: FreeRule,
    pub max_tokens: Option<u64>,
    /// The provider's model list leaves out some models we suggest (Z.ai omits its
    /// free ones), so suggestions are offered even when the live list lacks them.
    pub list_incomplete: bool,
    /// Extra headers the model-list endpoint needs.
    pub list_headers: &'static [(&'static str, &'static str)],
}

impl Entry {
    pub fn needs_key(&self) -> bool {
        !self.key_url.is_empty()
    }

    pub fn is_free(&self, model: &str, priced_free: bool) -> bool {
        match self.free {
            FreeRule::All => true,
            FreeRule::None => false,
            FreeRule::Contains(s) => model.contains(s),
            FreeRule::OpenRouter => {
                priced_free
                    || model.ends_with(":free")
                    || model == "openrouter/free"
                    || model == "kilo-auto/free"
            }
            FreeRule::Only(ids) => ids.contains(&model),
        }
    }
}

pub const ALL: &[Entry] = &[
    Entry {
        id: "zai",
        name: "Z.ai (GLM)",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.z.ai/api/paas/v4",
        key_url: "https://z.ai/manage-apikey/apikey-list",
        blurb: "Zhipu's GLM models. The Flash models are free and good at tool calling, which makes them a solid free base for a coding agent.",
        limits: "Only GLM-4.7-Flash and GLM-4.5-Flash are free. Newer Flash/FlashX models are paid and fail with \"insufficient balance\". Low concurrency (about one request at a time).",
        models: &["glm-4.7-flash", "glm-4.5-flash"],
        models_url: None,
        free: FreeRule::Only(&["glm-4.7-flash", "glm-4.5-flash", "glm-4.6v-flash"]),
        max_tokens: None,
        list_incomplete: true,
        list_headers: &[],
    },
    Entry {
        id: "openrouter",
        name: "OpenRouter",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://openrouter.ai/api/v1",
        key_url: "https://openrouter.ai/settings/keys",
        blurb: "One key for hundreds of models. Models ending in :free cost nothing; openrouter/free picks whichever free model is available.",
        limits: "Free models: 20 requests/min and 50/day, or 1000/day after a one-time $10 credit purchase. The free lineup changes often.",
        models: &["openrouter/free", "nvidia/nemotron-3-ultra-550b-a55b:free", "cohere/north-mini-code:free", "google/gemma-4-31b-it:free"],
        models_url: None,
        free: FreeRule::OpenRouter,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "groq",
        name: "Groq",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.groq.com/openai/v1",
        key_url: "https://console.groq.com/keys",
        blurb: "Extremely fast inference. Its free tier has small tokens-per-minute limits, so it works best as a fallback or for short requests.",
        limits: "Around 30 requests/min, 1000/day and 8K tokens/min per model. Long agent prompts often exceed the per-minute token limit.",
        models: &["openai/gpt-oss-120b", "openai/gpt-oss-20b"],
        models_url: None,
        free: FreeRule::All,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "nvidia",
        name: "NVIDIA NIM",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://integrate.api.nvidia.com/v1",
        key_url: "https://build.nvidia.com/settings/api-keys",
        blurb: "NVIDIA-hosted open models (DeepSeek, Kimi, Qwen, GLM, MiniMax and more), free for development use.",
        limits: "Free with an NVIDIA developer account. Around 40 requests/min.",
        models: &["z-ai/glm-5.3-flash", "deepseek-ai/deepseek-v4.1-flash", "moonshotai/kimi-k2.6", "minimaxai/minimax-m3.1-flash"],
        models_url: None,
        free: FreeRule::All,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "gemini",
        name: "Google Gemini",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        key_url: "https://aistudio.google.com/apikey",
        blurb: "Gemini Flash models through Google AI Studio, with a 1M-token context.",
        limits: "Flash models are free with small daily caps. Outside the EU/UK, free-tier prompts may be used for training.",
        models: &["gemini-flash-latest", "gemini-flash-lite-latest"],
        models_url: None,
        free: FreeRule::Contains("flash"),
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "kilo",
        name: "Kilo Gateway",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://ai-gateway.kilo.ai/api/v1",
        key_url: "",
        blurb: "Free coding models through Kilo Gateway. Anonymous access works for free models; prompts may be used for training by upstream providers.",
        limits: "Free models: 200 requests/hour per IP. Select kilo-auto/free or a model explicitly marked free. The lineup changes often.",
        models: &["kilo-auto/free", "cohere/north-mini-code:free"],
        models_url: None,
        free: FreeRule::OpenRouter,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "cloudflare",
        name: "Cloudflare Workers AI",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1",
        key_url: "https://dash.cloudflare.com/profile/api-tokens",
        blurb: "Open models served by Cloudflare. Create a token with Workers AI Read permission; setup also asks for your Cloudflare account ID.",
        limits: "10,000 neurons/day shared across models, resetting at 00:00 UTC. Some frontier models require payment; suggested models use the free allocation.",
        models: &["@cf/zai-org/glm-4.7-flash", "@cf/nvidia/nemotron-3-120b-a12b", "@cf/google/gemma-4-26b-a4b-it"],
        models_url: None,
        free: FreeRule::Only(&["@cf/zai-org/glm-4.7-flash", "@cf/nvidia/nemotron-3-120b-a12b", "@cf/google/gemma-4-26b-a4b-it"]),
        max_tokens: None,
        list_incomplete: true,
        list_headers: &[],
    },
    Entry {
        id: "llm7",
        name: "LLM7",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.llm7.io/v1",
        key_url: "https://dash.llm7.io/",
        blurb: "OpenAI-compatible educational and research service. Use a free token from the dashboard; only selected models accept its allowance.",
        limits: "Free token: up to 100,000 tokens per rolling 24 hours; 60 requests/min and 250/hour. Quotas and eligible models may change.",
        models: &["codestral-latest", "glm-5.3-flash", "gpt-oss:20b"],
        models_url: None,
        free: FreeRule::Only(&["DeepSeek-V4-Flash-0731", "codestral-latest", "glm-5.3-flash", "gpt-oss:20b", "minimax-m2.7", "mistral-Nemo-Instruct-2407"]),
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "typhoon",
        name: "Typhoon",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.opentyphoon.ai/v1",
        key_url: "https://playground.opentyphoon.ai/api-keys",
        blurb: "Thai and English models with free API access for light usage and prototyping. A secondary fallback rather than a primary coding model.",
        limits: "Listed text models: up to 5 requests/second and 200/minute. Availability and usage limits apply.",
        models: &["typhoon-v2.5-30b-a3b-instruct", "typhoon-v2.1-12b-instruct"],
        models_url: None,
        free: FreeRule::Only(&["typhoon-v2.5-30b-a3b-instruct", "typhoon-v2.1-12b-instruct"]),
        max_tokens: None,
        list_incomplete: true,
        list_headers: &[],
    },
    Entry {
        id: "cohere",
        name: "Cohere (evaluation)",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.cohere.ai/compatibility/v1",
        key_url: "https://dashboard.cohere.com/api-keys",
        blurb: "Command models through the OpenAI-compatible API. Trial keys provide free evaluation access; production and commercial use require a paid key.",
        limits: "Evaluation keys: 1,000 calls/month and generally 20 chat requests/minute. Paid keys are billed per token.",
        models: &["command-a-plus-05-2026", "command-a-03-2025"],
        models_url: Some("https://api.cohere.com/v1/models"),
        free: FreeRule::Only(&["command-a-plus-05-2026", "command-a-03-2025"]),
        max_tokens: None,
        list_incomplete: true,
        list_headers: &[],
    },
    Entry {
        id: "vercel",
        name: "Vercel AI Gateway",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://ai-gateway.vercel.sh/v1",
        key_url: "https://vercel.com/dashboard",
        blurb: "Monthly free credits for selected eligible models. Check Free Tier eligibility in the Vercel dashboard before selecting a model; models are not marked universally free here.",
        limits: "$5/month on the free tier; a valid payment method may be required. Purchasing credits moves the account to paid and ends the monthly free credit.",
        models: &[],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "mistral",
        name: "Mistral",
        tier: Tier::Paid,
        wire: Wire::Openai,
        base_url: "https://api.mistral.ai/v1",
        key_url: "https://console.mistral.ai/api-keys",
        blurb: "Mistral's API includes Codestral and Devstral, models built for coding. API access requires billing activation.",
        limits: "Free mode does not include API access. Creating API keys and using the API require billing activation and pay-as-you-go.",
        models: &["devstral-latest", "codestral-latest", "mistral-large-latest"],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "ollama",
        name: "Ollama",
        tier: Tier::Local,
        wire: Wire::Openai,
        base_url: "http://localhost:11434/v1",
        key_url: "",
        blurb: "Models running on your own machine. Free, private and offline. Pull one first, e.g. `ollama pull qwen3-coder`.",
        limits: "No limits beyond your hardware.",
        models: &[],
        models_url: None,
        free: FreeRule::All,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "lmstudio",
        name: "LM Studio",
        tier: Tier::Local,
        wire: Wire::Openai,
        base_url: "http://localhost:1234/v1",
        key_url: "",
        blurb: "Local models served by LM Studio. Start its server from the Developer tab first.",
        limits: "No limits beyond your hardware.",
        models: &[],
        models_url: None,
        free: FreeRule::All,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "deepseek",
        name: "DeepSeek",
        tier: Tier::Paid,
        wire: Wire::Openai,
        base_url: "https://api.deepseek.com",
        key_url: "https://platform.deepseek.com/api_keys",
        blurb: "DeepSeek's own API. Not free, but very cheap and strong at coding. Free DeepSeek models are also on NVIDIA NIM and OpenRouter.",
        limits: "Pay as you go, a few cents per million tokens. No rate limits to speak of.",
        models: &["deepseek-chat", "deepseek-reasoner"],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "zai-coding",
        name: "Z.ai GLM Coding Plan",
        tier: Tier::Paid,
        wire: Wire::Anthropic,
        base_url: "https://api.z.ai/api/anthropic",
        key_url: "https://z.ai/manage-apikey/apikey-list",
        blurb: "For GLM Coding Plan subscribers: the large GLM models at a flat monthly price, via Z.ai's Anthropic-compatible endpoint (best fit for Claude Code).",
        limits: "Usage limits per 5-hour window, depending on your plan.",
        models: &[],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
];

pub fn get(id: &str) -> Option<&'static Entry> {
    ALL.iter().find(|e| e.id == id)
}

pub fn cloudflare_url(account: &str) -> Result<String, &'static str> {
    if account.len() != 32 || !account.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Cloudflare account ID must be 32 hexadecimal characters (find it in your Cloudflare dashboard)");
    }
    Ok(get("cloudflare")
        .unwrap()
        .base_url
        .replace("{account_id}", account))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zai_flash_is_not_automatically_free() {
        let z = get("zai").unwrap();
        assert!(z.is_free("glm-4.7-flash", false));
        assert!(!z.is_free("glm-5.3-flash", false));
        assert!(!z.is_free("glm-5.3-flashx", false));
    }

    #[test]
    fn allowance_providers_do_not_mark_paid_models_free() {
        assert!(get("kilo").unwrap().is_free("kilo-auto/free", false));
        assert!(get("kilo")
            .unwrap()
            .is_free("cohere/north-mini-code:free", false));
        assert!(!get("kilo")
            .unwrap()
            .is_free("anthropic/claude-sonnet-4.5", false));
        assert!(get("llm7").unwrap().is_free("codestral-latest", false));
        assert!(!get("llm7").unwrap().is_free("Inkling-Small", false));
        assert!(!get("cloudflare")
            .unwrap()
            .is_free("@cf/zai-org/glm-5.3", false));
        assert!(!get("vercel").unwrap().is_free("openai/gpt-6", false));
        assert!(get("github").is_none());
    }

    #[test]
    fn cloudflare_account_id_cannot_change_url_structure() {
        let account = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            cloudflare_url(account).unwrap(),
            format!("https://api.cloudflare.com/client/v4/accounts/{account}/ai/v1")
        );
        assert!(cloudflare_url("short").is_err());
        assert!(cloudflare_url("../../../../somewhere/else/path!!").is_err());
    }

    #[test]
    fn suggestions_are_free_and_unique() {
        for e in ALL {
            for m in e.models {
                assert!(
                    e.tier != Tier::Free || e.is_free(m, false),
                    "{}/{m} suggested but not free",
                    e.id
                );
                assert_eq!(e.models.iter().filter(|x| *x == m).count(), 1);
            }
        }
    }

    #[test]
    fn test_model_skips_stale_suggestions() {
        use crate::tui::net::pick_test_model;
        let live = vec![
            ("baidu/x:free".to_string(), true),
            ("google/gemma-4-31b-it:free".to_string(), true),
        ];
        assert_eq!(
            pick_test_model("openrouter", &live).as_deref(),
            Some("google/gemma-4-31b-it:free")
        );
        let none_suggested = vec![
            ("paid/a".to_string(), false),
            ("free/b:free".to_string(), false),
        ];
        assert_eq!(
            pick_test_model("openrouter", &none_suggested).as_deref(),
            Some("free/b:free")
        );
    }
}
