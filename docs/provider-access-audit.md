# Provider access review — 16 November 2026

The initial audit checked model prices and model listings without proving that
a new account could obtain usable API access for free. That was insufficient.
This review separates account access, token pricing, and successful inference.
No accounts were created, identity checks submitted, or billing activated.

| Addition | Access finding | Tako behavior |
| --- | --- | --- |
| Nous Portal | Creating a Portal API key requires adding credits or starting a plan. A zero-priced model is not proof of free API-key access. Hermes OAuth is separate. | **PAID**. No Nous model enters `free:*`, even if its ID ends in `:free`. |
| ModelScope | Free inference requires linking an Alibaba Cloud account that has passed real-name verification. Current calls consume Magicube points: 0.5, 1, or 2 per model tier. The old 2,000 calls/day claim is removed. | Free allocation with explicit account/points requirements. No claim of unrestricted or globally available access. |
| SiliconFlow China | Selected models have zero input/output prices. Identity verification and supported documents can restrict eligibility; paid Pro variants are separate. No authenticated inference was tested. | Named free models only; guide links verification and pricing requirements. |
| BlockRun free endpoint | Public endpoint requires no key, account, wallet, or payment. A fresh anonymous request to Laguna XS 2.1 returned HTTP 200 with nonempty message content. Aliases can use another free upstream. | Free, with current upstream availability limits. |
| Novita | Apodex's model page lists zero token prices. The onboarding guide requires checking credit and model access. Zero-balance access to Apodex was not authenticated or proven. | **CHECK ACCESS**, excluded from `free:*`. Keep the promotion distinct from a verified free account. |
| Vikasit Nova | Official page advertises 2M free tokens/day and no card. Public model discovery timed out; no account/key was available to verify inference. | **CHECK ACCESS**, excluded from `free:*` until access is verified by the user. |

## First-party evidence

- [Nous comparison and API-key prerequisites](https://portal.nousresearch.com/compare):
  its FAQ distinguishes account/API-key credit requirements from free model prices
  and Hermes subscription usage.
- [ModelScope current usage restrictions](https://modelscope.cn/docs/model-service/API-Inference/limits):
  account linking, real-name verification, Magicube point deduction, and dynamic
  concurrency. The current source Markdown is served from the documentation
  prefix returned by `https://www.modelscope.cn/api/v1/document/main_doc_CN_prod`.
- [ModelScope source Markdown checked for this review](https://resouces.modelscope.cn/document/docdata/2026-11-10_17-29-CN/dist/model-service/API-Inference/limits/limits_CN.md).
- [SiliconFlow authentication requirements and eligible identity documents](https://docs.siliconflow.cn/docs/userguide/faqs/authentication),
  [current zero-price models](https://www.siliconflow.cn/pricing), and
  [signup/key creation guide](https://docs.siliconflow.cn/docs/userguide/quickstart).
- [BlockRun free endpoint and access terms](https://blockrun.ai/free).
- [Novita account/key/credit setup](https://docs.novita.ai/guides/quickstart) and
  [Apodex promotional price](https://novita.ai/models/model-detail/apodex-apodex-1.1-mini).
- [Vikasit Nova allowance and account instructions](https://vikasit.ai/inference).

## Setup completeness

All 22 catalog entries have three provider-specific setup steps in the web panel,
with guides available on both unconnected and connected provider cards. Connection
dialogs show the guide before saving a key. The guides identify account/billing
requirements, link the official provider pages, and include copyable API endpoints
and example Tako targets. Anonymous endpoints explicitly instruct leaving the
key blank. The terminal's provider details share the same setup steps.

The six new providers use their own bundled icons, recorded in
`assets/providers/sources.json`, instead of generated letter tiles. They work
without requesting images from third-party sites when the panel is opened.
