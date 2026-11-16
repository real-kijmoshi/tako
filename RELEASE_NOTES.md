# Tako 0.1.4

Tako now includes 22 providers, adding Nous Portal, ModelScope, SiliconFlow China,
BlockRun's anonymous free endpoint, Novita's free Apodex promotion, and Vikasit Nova.
Recommended models appear first in the web picker, with a free-only filter for
providers with identified free models. Quotas, promotions, and paid access are
explained in the catalog; Mistral's direct API is clearly labelled paid.

Smart routes such as `free:auto`, `free:coding`, and `any:reasoning` choose a model
for each request using task fit and observed health, then fall back through the
ranked candidates. Routes work as defaults and aliases and appear in the model
list, playground, and Combos screen. The live routing map now makes traffic and
in-flight requests easier to follow. The setup checklist can be dismissed.

## Free-model checks

Before release, live catalogs and current pricing documentation were rechecked.
Twelve tiny inference requests succeeded across Z.ai, Groq, Kilo, and BlockRun.
Unavailable BlockRun Muse Glimmer and retired Typhoon 2.1 were removed from the
recommendations. Other authenticated providers were checked against public
catalogs/documentation; their inference was not tested without credentials.
Free tiers remain account- and quota-dependent, and promotions can end.

See [provider selections and evidence](https://github.com/real-kijmoshi/tako/blob/v0.1.4/docs/providers.md)
and the [dated model audit](https://github.com/real-kijmoshi/tako/blob/v0.1.4/docs/free-model-audit.json).

Update with `tako update`. Updating restarts a running router and can interrupt
active requests; configuration and keys persist.
