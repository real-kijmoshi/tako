//! Built-in list of recommended providers, mostly ones with a free tier.
//!
//! Free tiers and model names change often, so the TUI fetches each provider's
//! live model list; `models` here are only the suggested picks.
//! Last checked: 16 November 2026. Sources and selection notes: docs/providers.md.

use crate::config::Wire;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Free,
    Local,
    Paid,
    Conditional,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Free => "FREE",
            Tier::Local => "LOCAL",
            Tier::Paid => "PAID",
            Tier::Conditional => "CHECK ACCESS",
        }
    }
}

/// Which of a provider's models cost nothing.
#[derive(Clone, Copy)]
pub enum FreeRule {
    All,
    None,
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

/// In-app setup instructions. Kept alongside the catalog for web and terminal clients.
pub struct SetupGuide {
    pub url: &'static str,
    pub steps: &'static [&'static str],
}

impl Entry {
    pub fn setup_guide(&self) -> SetupGuide {
        let (url, steps): (&str, &[&str]) = match self.id {
            "nous" => ("https://portal.nousresearch.com/compare", &["Sign in to Nous Portal. Read the API access requirements: creating an API key requires adding credits or starting a plan.", "Open API keys and create a Portal API key after enabling paid access. Hermes OAuth and plan-included usage are separate from API-key billing.", "Paste the Portal API key in Tako and connect. Test a model before adding it to a paid or any route; Nous is excluded from free routes."]),
            "modelscope" => ("https://modelscope.ai/posts/434362", &["Sign in to ModelScope, bind an Alibaba Cloud account, and complete that account's real-name verification. Then open your profile → Access Tokens and create an inference token.", "Choose a model whose API Inference provider is ModelScope. Commercial partners and Alibaba DashScope use different credentials and billing.", "Paste the ModelScope token in Tako, connect, then test a recommended text model. Check your Magicube points balance; calls cost 0.5, 1 or 2 points by model tier. If the balance is exhausted, obtain more points under the account rules."]),
            "siliconflow" => ("https://www.siliconflow.cn/pricing", &["Register on SiliconFlow China and check its real-name requirements. Online verification accepts specified Chinese/residence documents; contact support for other documents. International SiliconFlow accounts use a different service.", "Open Account → API Keys and create a key. Check the current pricing page: only models with zero input and output prices are free; Pro variants are paid.", "Paste the key in Tako, connect, and test a recommended zero-priced model. Keep Free only enabled when building your combo."]),
            "blockrun" => ("https://blockrun.ai/free", &["Use BlockRun’s public free endpoint. No account, API key, wallet or payment setup is needed for this endpoint.", "Leave API key blank and connect. Choose a listed free model; availability depends on upstream capacity and aliases may return a different free model.", "Run Test, then add working models to a combo or use free:auto. Paid BlockRun endpoints and wallet setup are separate services."]),
            "novita" => ("https://novita.ai/models/model-detail/apodex-apodex-1.1-mini", &["Sign in to Novita and open API Keys to create a key. A free model listing does not establish whether your account must activate billing.", "Check Apodex 1.1 Mini’s current input/output prices and your account’s access requirements before connecting. This is a temporary promotion; other models are metered.", "Paste the key, connect, and explicitly test apodex/apodex-1.1-mini. Do not add other Novita models to a free combo."]),
            "vikasit" => ("https://vikasit.ai/inference", &["Sign in with GitHub on Vikasit’s inference page and create an API key.", "Select vikasit-nova for the advertised daily free allowance. Other Vikasit and resold models are pay-as-you-go.", "Paste the key, connect, and test Nova. If live discovery is unavailable, Tako labels its catalog suggestion as unverified; test it before relying on the route."]),
            "zai" => ("https://docs.z.ai/guides/overview/pricing", &["Sign in to Z.ai’s international API platform and create an API key in API key management.", "Use the standard model API endpoint, not the GLM Coding Plan endpoint. GLM 4.7 Flash and 4.5 Flash are free; newer Flash and FlashX models are paid.", "Paste the key, connect, and test a suggested Flash model. Build a combo with one request at a time to respect concurrency limits."]),
            "openrouter" => ("https://openrouter.ai/settings/keys", &["Create an OpenRouter account, open Settings → API Keys, and create a key.", "Choose an explicit :free model or openrouter/free. A credit purchase is optional for higher free-model quotas; it is not needed for the basic daily free limit.", "Paste the key, connect, and browse Free only. Test your chosen model, then add it to a combo or use free:auto."]),
            "groq" => ("https://console.groq.com/docs/rate-limits", &["Sign in to GroqCloud Console and create a key on API Keys.", "Keep your account on the Free plan for free quotas. A Developer account uses paid pricing; inspect your console’s plan and model limits.", "Paste the key, connect, and test GPT OSS. Short requests work best within the free token-per-minute allowance."]),
            "nvidia" => ("https://build.nvidia.com", &["Sign in to NVIDIA Build with a developer account and open API Keys to generate a key.", "Choose an NVIDIA-hosted serverless model that your account can access. Free development access and limits are provider-controlled; self-hosted NIM infrastructure is separate.", "Paste the key, connect, and test a recommended coding model. Add working choices to your fallback combo."]),
            "gemini" => ("https://aistudio.google.com/docs/pricing", &["Open Google AI Studio, choose your project, and create an API key.", "Check that the project uses the Free tier and your model has a free quota. Paid-tier projects are billed at paid rates even for models with free-tier offerings.", "Paste the key, connect, and test a recommended Flash model. Region, project and daily limits apply."]),
            "kilo" => ("https://kilo.ai/docs/gateway/authentication", &["Use the Kilo Gateway free endpoint with the API key field blank. Anonymous free-model access is limited by IP.", "Select kilo-auto/free or an explicit :free model. Other gateway models require an account and paid access.", "Connect and run Test. Add working models to a combo; no Kilo key is required for the anonymous free pool."]),
            "cloudflare" => ("https://developers.cloudflare.com/workers-ai/get-started/rest-api/", &["Sign in to Cloudflare, open your account, and copy its 32-character Account ID.", "Create an API token with Workers AI Read permission for that account. Check model eligibility and the daily neuron allocation.", "Enter the Account ID and token in Tako; the account-specific endpoint is filled in for you. Connect and test an eligible model."]),
            "llm7" => ("https://dash.llm7.io/", &["Sign in to the LLM7 dashboard and create a Free API token.", "Check which models your token’s allowance covers. A model’s general catalog price is different from access within the Free-token allowance.", "Paste the token, connect, and test a suggested eligible model. Treat this educational service as a fallback with changing quotas."]),
            "typhoon" => ("https://docs.opentyphoon.ai/en/rate-limits/", &["Sign in to the OpenTyphoon playground and create an API key on API Keys.", "Choose the available Typhoon text model. OCR and speech model listings are not interchangeable with chat models.", "Paste the key, connect, and test Typhoon 2.5 30B. Respect the published per-model limits when adding it to a combo."]),
            "cohere" => ("https://docs.cohere.com/docs/rate-limits", &["Sign in to the Cohere dashboard and create an evaluation API key.", "Evaluation keys provide limited free testing; production keys are paid. Check the model and monthly allowance associated with your key.", "Paste the evaluation key, connect, and test a suggested Command model before adding it to an evaluation combo."]),
            "vercel" => ("https://vercel.com/docs/ai-gateway/pricing", &["Open your Vercel team’s AI Gateway dashboard and create a gateway key.", "Check Free Tier model eligibility and your account’s monthly credits. Purchasing credits transitions the team to paid access.", "Paste the key, connect, and choose an eligible model in the dashboard. Tako does not label arbitrary Vercel models free."]),
            "mistral" => ("https://docs.mistral.ai/admin/billing-usage/usage-limits", &["Sign in to Mistral Studio and activate billing/pay-as-you-go for API access. Studio or Vibe Free mode does not include API access.", "Create an API key in the console. Review rates and your spending limit before testing Codestral, Devstral or Mistral Large.", "Paste the key, connect, and use paid or any routes. For NVIDIA-hosted Mistral, configure NVIDIA separately with its own key."]),
            "ollama" => ("https://docs.ollama.com/api/openai-compatibility", &["Install Ollama and download a model that fits your hardware, for example with ollama pull qwen3-coder.", "Start Ollama’s local server. Leave the API key blank and use the local endpoint shown below.", "Connect, browse your installed models, and run Test. Models run on the machine hosting Tako."]),
            "lmstudio" => ("https://lmstudio.ai/docs/developer", &["Install LM Studio and download a model that fits the host machine.", "Open Developer → Local Server, load the model and start the OpenAI-compatible server on port 1234.", "Leave the key blank unless you enabled server authentication, connect, and test the loaded model."]),
            "deepseek" => ("https://platform.deepseek.com/api_keys", &["Sign in to DeepSeek’s developer platform and add credits for its paid API.", "Create an API key and review current model pricing and your account balance.", "Paste the key, connect, and test a model using a paid or any route. NVIDIA-hosted DeepSeek requires a separate NVIDIA key."]),
            "zai-coding" => ("https://docs.z.ai/devpack/overview", &["Subscribe to the GLM Coding Plan on Z.ai. Standard free Flash access is a separate provider.", "Create or copy the API key for your subscribed account. Tako uses the Coding Plan’s Anthropic-compatible endpoint.", "Paste the key, connect, and add models supported by your plan to a paid combo. Check the plan’s rolling usage limits."]),
            _ => (self.key_url, &["Read your provider's API documentation.", "Create a key with the required inference permissions and check its billing terms.", "Connect, test a model, then add it to a combo."]),
        };
        SetupGuide { url, steps }
    }
}

pub const ALL: &[Entry] = &[
    Entry {
        id: "nous",
        name: "Nous Portal (paid API access)",
        tier: Tier::Paid,
        wire: Wire::Openai,
        base_url: "https://inference-api.nousresearch.com/v1",
        key_url: "https://portal.nousresearch.com/api-keys",
        blurb: "Nous Portal API keys require adding credits or starting a plan. Zero-priced model variants do not make the API-key setup free. Hermes Portal sign-in is a separate access method.",
        limits: "Payment is required to obtain a Portal API key. API calls use credit balances, not a plan's included Hermes usage. Tako does not implement Portal OAuth; Nous targets are excluded from free routes even when model IDs end in :free.",
        models: &["meituan/longcat-2.5:free", "stepfun/step-3.7-flash:free", "upstage/solar-mini4:free"],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "modelscope",
        name: "ModelScope (verified account)",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api-inference.modelscope.cn/v1",
        key_url: "https://modelscope.cn/my/myaccesstoken",
        blurb: "Free ModelScope-hosted inference, including DeepSeek, Qwen, GLM, Kimi and StepFun. Use a ModelScope access token, not an Alibaba DashScope key.",
        limits: "Requires a linked, real-name-verified Alibaba Cloud account. Calls spend your available Magicube points (0.5, 1 or 2 per call by model tier); check current balance and eligibility. Single-request concurrency is recommended. Commercial partner endpoints are separate.",
        models: &["deepseek-ai/DeepSeek-V4.1-Flash", "ZhipuAI/GLM-5.2", "stepfun-ai/Step-5-Preview", "Qwen/Qwen3.8-27B", "moonshotai/Kimi-K3.1-Pro", "MiniMax/MiniMax-M3"],
        models_url: None,
        free: FreeRule::All,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "siliconflow",
        name: "SiliconFlow (China)",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://api.siliconflow.cn/v1",
        key_url: "https://cloud.siliconflow.cn/account/ak",
        blurb: "Selected small open models have zero input and output prices. Xing4.0 is the larger reasoning choice; Qwen3 8B and Qwen3.5 4B are lighter fallbacks.",
        limits: "Check identity and regional eligibility for the China service. Online verification supports specified Chinese/residence documents; other documents require support. Only named zero-priced models are free; Pro variants are paid. Free-model rate limits apply.",
        models: &["XingChenAGI/Xing4.0-29B", "Qwen/Qwen3-8B", "Qwen/Qwen3.5-4B", "deepseek-ai/DeepSeek-R1-0528-Qwen3-8B", "THUDM/GLM-4-9B-0414", "THUDM/GLM-Z1-9B-0414", "Qwen/Qwen2.5-7B-Instruct", "tencent/Hunyuan-MT-7B"],
        models_url: None,
        free: FreeRule::Only(&["XingChenAGI/Xing4.0-29B", "Qwen/Qwen3-8B", "Qwen/Qwen3.5-4B", "deepseek-ai/DeepSeek-R1-0528-Qwen3-8B", "THUDM/GLM-4-9B-0414", "THUDM/GLM-Z1-9B-0414", "Qwen/Qwen2.5-7B-Instruct", "tencent/Hunyuan-MT-7B"]),
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "blockrun",
        name: "BlockRun (free endpoint)",
        tier: Tier::Free,
        wire: Wire::Openai,
        base_url: "https://blockrun.ai/api/v1",
        key_url: "",
        blurb: "Anonymous free inference with Nemotron reasoning models, Laguna coding and Muse Glimmer. Use this free endpoint without a key or wallet.",
        limits: "Fair-use and upstream availability limits apply. Unavailable models are hidden from the live picker. Paid BlockRun services use separate billing; free model aliases may fall back to another healthy free model.",
        models: &["poolside/laguna-xs-2.1", "nvidia/nemotron-3-ultra-550b", "nvidia/nemotron-3.5-lightning", "nvidia/nemotron-3.5-bolt"],
        models_url: None,
        free: FreeRule::Only(&["poolside/laguna-xs-2.1", "nvidia/nemotron-3-ultra-550b", "nvidia/nemotron-3.5-lightning", "nvidia/muse-glimmer-30b", "nvidia/nemotron-3.5-bolt"]),
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "novita",
        name: "Novita (Apodex promotion)",
        tier: Tier::Conditional,
        wire: Wire::Openai,
        base_url: "https://api.novita.ai/openai/v1",
        key_url: "https://novita.ai/settings/key-management",
        blurb: "Apodex 1.1 Mini is a coding and tool-use model with a 256K context, currently listed at zero input and output cost. Other Novita models are metered.",
        limits: "Apodex has a zero-price promotion, but Novita requires checking account credit and model access. No authenticated zero-balance test was completed. Excluded from automatic free routes until you verify account access.",
        models: &["apodex/apodex-1.1-mini"],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: false,
        list_headers: &[],
    },
    Entry {
        id: "vikasit",
        name: "Vikasit AI (Nova allowance)",
        tier: Tier::Conditional,
        wire: Wire::Openai,
        base_url: "https://api.vikasit.ai/v1",
        key_url: "https://vikasit.ai/inference",
        blurb: "Vikasit Nova offers a daily free allowance for general chat and coding. Other Vikasit and ecosystem models are pay-as-you-go.",
        limits: "Provider advertises Nova at 2 million tokens/day with no card required. Public model discovery timed out and inference was not verified. Excluded from automatic free routes; verify access before using it.",
        models: &["vikasit-nova"],
        models_url: None,
        free: FreeRule::None,
        max_tokens: None,
        list_incomplete: true,
        list_headers: &[],
    },
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
        models: &["inclusionai/ling-3.2-flash:free", "cohere/north-mini-code:free", "nvidia/nemotron-3-ultra-550b-a55b:free", "nvidia/nemotron-3.5-super-vl:free", "thinkingmachines/inkling-2-small:free", "google/gemma-4-31b-it:free", "openrouter/free"],
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
        models: &["deepseek-ai/deepseek-v4.1-flash", "z-ai/glm-5.3-flash", "minimaxai/minimax-m3.1-flash", "stepfun-ai/step-5-preview", "mistralai/mistral-large-4-1t-a52b", "nvidia/nemotron-3-ultra-550b-a55b", "poolside/laguna-xs-2.1"],
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
        blurb: "Gemini through Google AI Studio. Gemini 3.8 Flash is the general-purpose pick; Flash-Lite is the quick fallback. Only documented free-tier text models are marked free.",
        limits: "Free-tier quotas depend on the model, project and region. Paid-tier projects are billed at paid rates. Omni Flash and image/audio-only models are not automatically free chat models. Free-tier data terms apply.",
        models: &["gemini-3.8-flash", "gemini-3.5-flash-lite", "gemini-3.1-flash-lite"],
        models_url: None,
        free: FreeRule::Only(&["gemini-3.8-flash", "gemini-3.6-flash", "gemini-3.5-flash-lite", "gemini-3.1-flash-lite", "gemini-3-flash-preview", "gemini-2.5-flash", "gemini-2.5-flash-lite"]),
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
        models: &["cohere/north-mini-code:free", "nvidia/nemotron-3-ultra-550b-a55b:free", "nvidia/nemotron-3.5-super-vl:free", "kilo-auto/free"],
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
        models: &["typhoon-v2.5-30b-a3b-instruct"],
        models_url: None,
        free: FreeRule::Only(&["typhoon-v2.5-30b-a3b-instruct"]),
        max_tokens: None,
        list_incomplete: false,
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
        name: "Mistral (paid API)",
        tier: Tier::Paid,
        wire: Wire::Openai,
        base_url: "https://api.mistral.ai/v1",
        key_url: "https://console.mistral.ai/api-keys",
        blurb: "Mistral's API includes Codestral and Devstral, models built for coding. API access requires billing activation. For a free hosted Mistral option, connect NVIDIA NIM instead.",
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
    fn free_access_is_specific_to_the_provider_and_model() {
        assert!(!get("nous")
            .unwrap()
            .is_free("upstage/solar-mini4:free", true));
        assert!(!get("nous").unwrap().is_free("upstage/solar-mini4", false));
        assert!(get("siliconflow").unwrap().is_free("Qwen/Qwen3-8B", false));
        assert!(!get("siliconflow")
            .unwrap()
            .is_free("Pro/Qwen/Qwen3-8B", false));
        assert!(!get("vikasit").unwrap().is_free("vikasit-nova", false));
        assert!(!get("vikasit").unwrap().is_free("vikasit-3", false));
        assert!(!get("novita")
            .unwrap()
            .is_free("apodex/apodex-1.1-mini", false));
        assert!(!get("novita")
            .unwrap()
            .is_free("deepseek/deepseek-v4.1-flash", false));
        assert!(get("gemini").unwrap().is_free("gemini-3.8-flash", false));
        assert!(!get("gemini")
            .unwrap()
            .is_free("gemini-omni-1.2-flash", false));
        let mistral = get("mistral").unwrap();
        assert!(mistral.tier == Tier::Paid);
        assert!(!mistral.is_free("codestral-latest", true));
        assert!(!get("blockrun").unwrap().needs_key());
        assert!(!get("blockrun")
            .unwrap()
            .is_free("anthropic/claude-opus", false));
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
    fn every_provider_has_actionable_setup_steps() {
        for entry in ALL {
            let guide = entry.setup_guide();
            assert!(guide.url.starts_with("https://"), "{}", entry.id);
            assert!(guide.steps.len() >= 3, "{}", entry.id);
            assert!(guide.steps.iter().all(|step| step.len() > 40));
        }
        assert!(get("nous").unwrap().setup_guide().steps[0].contains("credits"));
        assert!(get("modelscope").unwrap().setup_guide().steps[0].contains("real-name"));
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
