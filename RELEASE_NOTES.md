# Tako 0.1.6

This patch corrects the free-access claims in 0.1.5 and completes provider setup.

- **Nous Portal is paid API access in Tako.** Creating a Portal key requires adding credits or starting a plan. Nous is excluded from free routes, including models with `:free` IDs. Hermes OAuth is a separate access method.
- **Novita and Vikasit are marked Check access.** Novita’s zero-balance promotional access was not verified; Vikasit’s advertised free allowance could not be tested. Neither enters automatic free routes.
- **ModelScope requirements are explicit.** Link a verified Alibaba Cloud account and check Magicube points; the stale 2,000 calls/day claim is removed. SiliconFlow’s identity and eligibility limits are also documented.
- **Every provider has an in-app setup guide**, with account steps, official links, API key instructions, a copyable endpoint, and a first target to test. Guides are available before and after connecting.
- **The six new providers have bundled official icons**, available without external image requests. Connected cards show their access classification.

BlockRun’s anonymous free endpoint was rechecked with a successful response containing nonempty content. No accounts were created or billing activated for this review.

See the [account-access review](https://github.com/real-kijmoshi/tako/blob/v0.1.6/docs/provider-access-audit.md) for sources and the distinction between model price and usable account access.

Update with `tako update`. Updating restarts a running router and can interrupt active requests; configuration and keys persist. Existing Nous, Novita, or Vikasit targets remain in your configuration, but `free:*` routes exclude them.
