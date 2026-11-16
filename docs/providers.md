# Free providers and recommended models

Checked 16 November 2026. Recommendations prioritize coding, tool use, reasoning,
and useful fallback coverage; they are curated picks, not benchmark rankings.
The model picker loads the full live catalog, shows recommendations first, and
starts with **Free only** for providers with identified free models and local providers. Uncheck it to see
paid models. Connecting a provider still requires your own account/key unless
the catalog says no key is needed.

| Provider | Recommended starting points | What is free |
| --- | --- | --- |
| Nous Portal | LongCat 2.5, Step 3.7 Flash, Solar Mini 4 | Explicit `:free` variants; other models use credits |
| OpenRouter | Ling 3.2 Flash, North Mini Code, Nemotron 3 Ultra, Nemotron 3.5 Super VL, Inkling 2 Small, Gemma 4 31B | `:free` models and `openrouter/free`; daily/minute limits apply |
| NVIDIA NIM | DeepSeek V4.1 Flash, GLM 5.3 Flash, MiniMax M3.1 Flash, Step 5 Preview, Mistral Large 4 | Developer access, subject to NVIDIA account limits |
| ModelScope | DeepSeek V4.1 Flash, GLM 5.2, Step 5 Preview, Qwen3.8 27B, Kimi K3.1 Pro, MiniMax M3 | ModelScope-hosted daily inference allowance, not commercial partner endpoints |
| Kilo Gateway | North Mini Code, Nemotron 3 Ultra, Nemotron 3.5 Super VL, `kilo-auto/free` | Explicit free models; anonymous access has IP limits |
| SiliconFlow China | Xing4.0 29B, Qwen3 8B, Qwen3.5 4B, DeepSeek R1 distilled 8B | Named zero-priced models only; Pro variants are paid |
| BlockRun free endpoint | Laguna XS 2.1, Nemotron 3 Ultra, Nemotron 3.5 Lightning, Nemotron 3.5 Bolt | Named free models without a key/wallet; unavailable models are hidden; aliases may use a different healthy free upstream |
| Novita | Apodex 1.1 Mini | Temporary zero-priced promotion for that model only |
| Vikasit | Vikasit Nova | Nova's 2M tokens/day allowance; other models are paid |
| Groq | GPT OSS 120B, GPT OSS 20B | Account free tier; short token-per-minute budgets |
| Gemini | Gemini 3.8 Flash, Gemini 3.5 Flash-Lite, Gemini 3.1 Flash-Lite | Documented free-tier text models; paid-tier keys are billed at paid-tier prices |
| Z.ai | GLM 4.7 Flash, GLM 4.5 Flash | Explicit free Flash IDs; newer Flash/FlashX names are not automatically free |
| Cloudflare | GLM 4.7 Flash, Nemotron 3 120B, Gemma 4 26B | Eligible models within daily neuron allocation |
| Cohere | Command A Plus, Command A | Evaluation key allowance; production access is paid |
| LLM7 | Codestral, GLM 5.3 Flash, GPT OSS 20B | Named models within the free-token allowance |
| Typhoon | Typhoon 2.5 30B | Limited API access; useful for Thai/English workloads |
| Vercel | Choose an eligible model in the dashboard | Monthly account credits; models are not universally zero-priced |
| Ollama / LM Studio | Choose a model that fits your hardware | Local inference; no hosted API quota |

Tako does not create accounts, add credits, or activate billing. A **free tier**
badge means the provider offers free access under its terms, not unlimited
requests or zero billing on a paid account. Check quotas and promotion status
before using a route for unattended workloads.

Live lists are preferred over suggestions. Providers known to omit models can
also show catalog suggestions. If their live list fails, the web picker labels
the suggestions as unverified instead of presenting them as a successful fetch.
Model list prices accept string and numeric zero values; missing prices and
nonzero request fees do not count as zero-priced. Explicitly unavailable models
and models declaring non-text-only output are excluded from chat discovery.

## Mistral and services excluded from the free catalog

Mistral's current [usage documentation](https://docs.mistral.ai/admin/billing-usage/usage-limits)
says Free mode covers Vibe and Studio, while API keys/API usage require billing
activation and pay-as-you-go. Tako therefore labels the direct Mistral API
**PAID**, including Codestral and Devstral. NVIDIA's hosted Mistral Large model
is a separate developer offering and is included under NVIDIA.

OpenCode's [current model documentation](https://opencode.ai/docs/console/models/)
restricts its free-tier chat models to the OpenCode client. It is not offered as
a generic free proxy provider. Cerebras currently offers a
[time-limited trial](https://inference-docs.cerebras.ai/support/rate-limits),
SambaNova's [Free plan requires purchased credits](https://cloud.sambanova.ai/plans),
and Hugging Face's [free accounts receive no inference credits](https://huggingface.co/docs/inference-providers/pricing).
These are not recurring free API options.

## Sources for the additions and refreshed picks

- Nous: [live catalog](https://inference-api.nousresearch.com/v1/models),
  [free offers](https://portal.nousresearch.com/models),
  [API key pricing and subscription distinction](https://portal.nousresearch.com/compare).
- OpenRouter: [live models and prices](https://openrouter.ai/api/v1/models).
- NVIDIA: [live model IDs](https://integrate.api.nvidia.com/v1/models).
- Kilo: [live gateway models](https://ai-gateway.kilo.ai/api/v1/models).
- ModelScope: [live model IDs](https://api-inference.modelscope.cn/v1/models),
  [official onboarding and daily quotas](https://modelscope.ai/posts/434362).
- SiliconFlow: [official price list](https://www.siliconflow.cn/pricing),
  including the zero-priced models in its embedded catalog data.
- BlockRun: [free endpoint and fallback behavior](https://blockrun.ai/free),
  [live availability and billing modes](https://blockrun.ai/api/v1/models).
- Novita: [Apodex model pricing](https://novita.ai/models/model-detail/apodex-apodex-1.1-mini),
  [official API quickstart and promotion terms](https://blogs.novita.ai/apodex-1-1-mini-on-novita-ai-api-quickstart-pricing/).
- Vikasit: [official Nova allowance, model ID and endpoint](https://vikasit.ai/inference).
- Gemini: [official per-model free and paid pricing](https://aistudio.google.com/docs/pricing).

Public catalogs were fetched without credentials. Documentation checks and model
discovery do not verify an individual account's quota or authenticated inference.

## Release 0.1.4 verification

The [dated audit](free-model-audit.json) records a fresh pre-release check of
recommended model listings, token prices where supplied, and tiny inference
requests. Twelve requests succeeded: both Z.ai Flash picks, both Groq GPT OSS
picks, all four Kilo recommendations and all four current BlockRun recommendations.
BlockRun can return a different healthy free model; the audit records that model.
Muse Glimmer was unavailable and has been removed from BlockRun recommendations.
Typhoon 2.1 was absent from the live list and has been removed from recommendations.

Nous's three free variants and OpenRouter's seven recommendations have zero
input/output token prices in their live catalogs. SiliconFlow's eight selected
models have zero prompt/completion prices in its current pricing-page data.
Novita's current Apodex page lists zero input/output prices. ModelScope and NVIDIA
list the selected models, but their lists do not establish account pricing.
Groq and LLM7 publish paid rates as well as free account allowances; their paid
prices are not interpreted as evidence that a free account has been billed.

No local credentials were available for the remaining authenticated providers.
Their catalog/documentation checks are recorded separately from successful
inference. Gemini's OpenAI-compatible listing endpoint matches its current
[official documentation](https://aistudio.google.com/docs/openai#list-models);
an anonymous 404 does not establish a failure for an authenticated account.
