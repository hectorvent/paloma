# `paloma commerce`

Control whether the shopping cart and product catalog appear for the active business
phone number. This uses the same endpoint as the `Commerce` manifest kind.

```bash
paloma commerce get -o json
paloma commerce set --cart-enabled true --catalog-visible true --dry-run
paloma commerce set --catalog-visible false --yes
```

`get` preserves the `/data` response and renders cart/catalog columns in table mode;
`-q` prints its settings ID when present. `set` requires at least one of
`--cart-enabled <true|false>` or `--catalog-visible <true|false>`. Omitted flags leave
the corresponding setting unchanged. Explicit false values are sent as query
parameters, as Meta requires. Disabling either feature confirms before writing;
`--yes` skips that prompt, and `--dry-run` previews without sending.

Hiding the catalog can invalidate catalog links and buttons from customers' point of
view. Repeated settings writes have no primary ID, so `set -q` prints nothing.
Invalid local input exits `5`; Graph failures use the standard exit-code mapping.
